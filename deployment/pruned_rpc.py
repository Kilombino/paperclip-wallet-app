#!/usr/bin/env python3
"""Private indexed RPC adapter for a validated, pruned XBT Knots node.

No wallet RPC is forwarded. The durable index covers every block from its
configured origin; it is not advertised as Bitcoin Core's global txindex.
"""
import argparse
import base64
import hmac
import json
import os
from pathlib import Path
import sqlite3
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

XBT_ACTIVATION = 961640
MAX_BODY = 8 * 1024 * 1024


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


class Node:
    def __init__(self, url, cookie):
        self.url, self.cookie = url, Path(cookie)

    def call(self, method, params=()):
        token = base64.b64encode(self.cookie.read_bytes().strip()).decode()
        data = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': list(params)}).encode()
        request = urllib.request.Request(self.url, data, {
            'Authorization': 'Basic ' + token, 'Content-Type': 'application/json'})
        try:
            response = urllib.request.urlopen(request, timeout=20)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            value = json.load(response)
        if value.get('error'):
            raise RpcError(value['error']['code'], value['error']['message'])
        return value['result']


class Index:
    def __init__(self, path, node, first_height, fetch_timeout=8):
        if first_height < 0:
            raise ValueError('Negative index origin')
        self.node, self.first, self.fetch_timeout = node, first_height, fetch_timeout
        self.lock = threading.RLock()
        self.fetch_lock = threading.Lock()
        self.db = sqlite3.connect(path, check_same_thread=False)
        self.db.execute('PRAGMA journal_mode=WAL')
        self.db.execute('PRAGMA synchronous=FULL')
        self.db.execute('PRAGMA foreign_keys=ON')
        self.db.executescript('''
            CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS block (height INTEGER PRIMARY KEY, hash TEXT UNIQUE NOT NULL);
            CREATE TABLE IF NOT EXISTS tx (txid TEXT NOT NULL, height INTEGER NOT NULL
                REFERENCES block(height) ON DELETE CASCADE, PRIMARY KEY(txid,height));
            CREATE INDEX IF NOT EXISTS tx_height ON tx(height);
            CREATE TABLE IF NOT EXISTS raw_cache (txid TEXT PRIMARY KEY, blockhash TEXT NOT NULL, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS scoped_tx (txid TEXT PRIMARY KEY, anchor_height INTEGER NOT NULL, anchor_hash TEXT NOT NULL);
        ''')
        genesis = self.node.call('getblockhash', [0])
        identity = json.dumps([first_height, genesis])
        existing = self.db.execute("SELECT value FROM metadata WHERE key='identity'").fetchone()
        if existing and existing[0] != identity:
            self.db.close()
            raise ValueError('Index origin or genesis differs from saved database')
        with self.db:
            self.db.execute("INSERT OR IGNORE INTO metadata VALUES ('identity',?)", [identity])
        self.genesis = genesis

    def tip(self):
        return self.db.execute('SELECT height,hash FROM block ORDER BY height DESC LIMIT 1').fetchone()

    def fetch_block(self, blockhash, verbosity=1):
        """Let Knots retrieve and validate a known block; never trust peer JSON."""
        try:
            return self.node.call('getblock', [blockhash, verbosity])
        except RpcError as error:
            if error.code != -1 or 'pruned' not in str(error).lower():
                raise
        # A header known to our node is required before requesting any data.
        self.node.call('getblockheader', [blockhash, True])
        with self.fetch_lock:
            peers = self.node.call('getpeerinfo')
            peers = [p for p in peers if int(p.get('services', '0'), 16) & 9 == 9]
            for peer in peers[:3]:
                try:
                    self.node.call('getblockfrompeer', [blockhash, peer['id']])
                except RpcError:
                    continue
                deadline = time.monotonic() + self.fetch_timeout
                while time.monotonic() < deadline:
                    try:
                        return self.node.call('getblock', [blockhash, verbosity])
                    except RpcError as error:
                        if error.code != -1 or 'pruned' not in str(error).lower():
                            raise
                    time.sleep(0.2)
        raise RpcError(-28, 'Historical block unavailable; index/lookup is not complete')

    def sync(self, limit=128):
        with self.lock:
            chain = self.node.call('getblockchaininfo')
            if chain.get('initialblockdownload', True):
                raise RpcError(-28, 'Backend is synchronizing')
            tip = self.tip()
            # Roll back atomically before inserting the replacement branch.
            while tip and (tip[0] > chain['blocks'] or self.node.call('getblockhash', [tip[0]]) != tip[1]):
                with self.db:
                    self.db.execute('DELETE FROM raw_cache WHERE blockhash=?', [tip[1]])
                    self.db.execute('DELETE FROM block WHERE height=?', [tip[0]])
                tip = self.tip()
            start = tip[0] + 1 if tip else self.first
            for height in range(start, min(chain['blocks'] + 1, start + limit)):
                blockhash = self.node.call('getblockhash', [height])
                block = self.fetch_block(blockhash)
                if block['hash'] != blockhash or block['height'] != height:
                    raise RpcError(-28, 'Backend returned a different block')
                tip = self.tip()
                if tip and block.get('previousblockhash') != tip[1]:
                    raise RpcError(-28, 'Chain changed during index update')
                if self.node.call('getblockhash', [height]) != blockhash:
                    raise RpcError(-28, 'Chain changed during block retrieval')
                with self.db:
                    self.db.execute('INSERT INTO block VALUES (?,?)', [height, blockhash])
                    self.db.executemany('INSERT INTO tx VALUES (?,?)', [(txid, height) for txid in block['tx']])
            return self.info()

    def info(self):
        with self.lock:
            chain = self.node.call('getblockchaininfo')
            tip = self.tip()
            return {'version': 1, 'first_height': self.first, 'genesis': self.genesis,
                    'best_block_height': tip[0] if tip else self.first - 1,
                    'best_block_hash': tip[1] if tip else None,
                    'synced': bool(tip and not chain.get('initialblockdownload', True)
                                   and tip[0] == chain['blocks'] and tip[1] == chain['bestblockhash']),
                    'missing_blocks': 'knots-peer-fetch', 'unknown_transactions': 'error'}

    def transaction(self, txid, verbose=False, blockhash=None):
        with self.lock:
            self.sync()
            if not self.info()['synced']:
                raise RpcError(-28, 'Transaction index is behind the active chain')
            if blockhash is None:
                try:
                    # Without txindex this covers the live mempool.
                    value = self.node.call('getrawtransaction', [txid, True])
                    if not value.get('blockhash'):
                        return value if verbose else value['hex']
                except RpcError as error:
                    if error.code != -5:
                        raise
                row = self.db.execute('SELECT block.hash FROM tx JOIN block USING(height) WHERE txid=? ORDER BY height DESC LIMIT 1', [txid]).fetchone()
                if not row:
                    if not self.info()['synced']:
                        raise RpcError(-28, 'Chain changed during transaction lookup')
                    if self.scope(txid):
                        raise RpcError(-5, 'Transaction absent from its proven covered history and mempool')
                    # A scoped index cannot prove absence before its origin.
                    raise RpcError(-32004, 'Transaction is not in indexed history or mempool; absence is not proven')
                blockhash = row[0]
            header = self.node.call('getblockheader', [blockhash, True])
            if header.get('confirmations', -1) <= 0:
                raise RpcError(-28, 'Transaction block is no longer in the active chain')
            cached = self.db.execute('SELECT value FROM raw_cache WHERE txid=? AND blockhash=?', [txid, blockhash]).fetchone()
            if cached:
                value = json.loads(cached[0])
            else:
                self.fetch_block(blockhash)
                value = self.node.call('getrawtransaction', [txid, True, blockhash])
                if value['txid'] != txid or value.get('blockhash') != blockhash:
                    raise RpcError(-28, 'Transaction does not match the requested block')
                with self.db:
                    self.db.execute('INSERT OR REPLACE INTO raw_cache VALUES (?,?,?)', [txid, blockhash, json.dumps(value)])
            current = self.node.call('getblockheader', [blockhash, True])
            if current.get('confirmations', -1) <= 0:
                raise RpcError(-28, 'Chain changed during transaction lookup')
            value['confirmations'] = current['confirmations']
            value['in_active_chain'] = True
            return value if verbose else value['hex']

    def scope(self, txid):
        """An active ancestor proves this transaction cannot predate coverage."""
        row = self.db.execute('SELECT height,hash FROM tx JOIN block USING(height) WHERE txid=? ORDER BY height LIMIT 1', [txid]).fetchone()
        if not row:
            row = self.db.execute('SELECT anchor_height,anchor_hash FROM scoped_tx WHERE txid=?', [txid]).fetchone()
        if row and self.node.call('getblockhash', [row[0]]) == row[1]:
            return row
        return None

    def register(self, raw_transactions):
        """Register recovery ancestry, without broadcasting or signing anything."""
        if not isinstance(raw_transactions, list) or not 0 < len(raw_transactions) <= 512:
            raise RpcError(-32602, 'Provide 1 to 512 raw transactions')
        with self.lock:
            if not self.sync()['synced']:
                raise RpcError(-28, 'Index is not ready')
            pending = [self.node.call('decoderawtransaction', [raw]) for raw in raw_transactions]
            # Packages may be supplied in reverse dependency order.
            while pending:
                remaining = []
                for tx in pending:
                    if self.scope(tx['txid']):
                        continue
                    anchor = next((bound for vin in tx['vin'] if 'txid' in vin
                                   if (bound := self.scope(vin['txid']))), None)
                    if anchor:
                        with self.db:
                            self.db.execute('INSERT OR REPLACE INTO scoped_tx VALUES (?,?,?)', [tx['txid'], *anchor])
                    else:
                        remaining.append(tx)
                if len(remaining) == len(pending):
                    raise RpcError(-32004, 'Recovery ancestry is outside covered active history')
                pending = remaining
            return True


FORWARD = frozenset('getblockchaininfo getnetworkinfo getbestblockhash getblockcount getblockhash getblockheader getindexinfo gettxout getrawmempool getmempoolentry getmempoolinfo getmempoolancestors getmempooldescendants gettxspendingprevout estimatesmartfee estimaterawfee getdeploymentinfo testmempoolaccept sendrawtransaction submitpackage'.split())


def dispatch(index, method, params):
    if not isinstance(params, list):
        raise RpcError(-32602, 'Positional parameters required')
    if method == 'getpaperclipindexinfo':
        return index.info()
    if method == 'getrawtransaction':
        return index.transaction(*params)
    if method == 'paperclipregistertransactions':
        return index.register(*params)
    if method == 'getblock':
        return index.fetch_block(*params)
    if method in FORWARD:
        return index.node.call(method, params)
    raise RpcError(-32601, 'RPC method not allowed')


def make_server(index, bind, port, cookie):
    expected = 'Basic ' + base64.b64encode(Path(cookie).read_bytes().strip()).decode()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'
        disable_nagle_algorithm = True
        def log_message(self, *args):
            pass

        def do_POST(self):
            self.connection.settimeout(30)
            if not hmac.compare_digest(self.headers.get('Authorization', ''), expected):
                self.send_error(401)
                return
            if self.path != '/' or self.headers.get('Transfer-Encoding'):
                self.send_error(400)
                return
            identifier = None
            try:
                size = int(self.headers.get('Content-Length', '0'))
                if not 0 < size <= MAX_BODY:
                    self.send_error(413)
                    return
                req = json.loads(self.rfile.read(size))
                identifier = req.get('id')
                result = dispatch(index, req['method'], req.get('params', []))
                response = {'result': result, 'error': None, 'id': identifier}
            except RpcError as error:
                response = {'result': None, 'error': {'code': error.code, 'message': str(error)}, 'id': identifier}
            except (ValueError, KeyError, TypeError, AttributeError):
                response = {'result': None, 'error': {'code': -32602, 'message': 'Invalid request'}, 'id': identifier}
            except Exception:
                response = {'result': None, 'error': {'code': -28, 'message': 'Chain adapter unavailable'}, 'id': identifier}
            payload = json.dumps(response).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    return ThreadingHTTPServer((bind, port), Handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--node-url', required=True)
    parser.add_argument('--node-cookie', required=True)
    parser.add_argument('--client-cookie', required=True)
    parser.add_argument('--database', required=True)
    parser.add_argument('--first-height', type=int, required=True)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--port', type=int, default=18336)
    parser.add_argument('--regtest', action='store_true')
    args = parser.parse_args()
    os.umask(0o077)
    node = Node(args.node_url, args.node_cookie)
    chain = node.call('getblockchaininfo')
    if args.regtest:
        if chain['chain'] != 'regtest' or args.first_height != 0:
            parser.error('Regtest requires its complete history from height zero')
    else:
        header = node.call('getblockheader', [chain['bestblockhash'], False])
        if chain['chain'] != 'main' or len(header) != 328 or args.first_height > XBT_ACTIVATION:
            parser.error('Require activated XBT mainnet and index origin no later than activation')
    index = Index(args.database, node, args.first_height)

    def worker():
        while True:
            try:
                # Release the index lock between historical blocks so health
                # checks do not wait behind an entire bootstrap batch.
                info = index.sync(limit=1)
                time.sleep(1 if info['synced'] else 0.01)
            except Exception:
                # No raw RPC errors or private request data in service logs.
                print('Index unavailable; retrying', flush=True)
                time.sleep(5)

    threading.Thread(target=worker, daemon=True).start()
    make_server(index, args.bind, args.port, args.client_cookie).serve_forever()


if __name__ == '__main__':
    main()
