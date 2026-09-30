#!/usr/bin/env python3
"""Optional private chain adapter supervisor for the wallet container."""
import argparse
import getpass
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import time
import tomllib
from urllib.parse import urlsplit

ROOT = Path('/data/chain')
LIB = Path('/usr/local/lib/paperclip')


def validate(config):
    if config.get('network') not in ('main', 'regtest'):
        raise ValueError('Choose XBT mainnet or isolated regtest')
    url = urlsplit(config.get('rpc_url', ''))
    if url.scheme not in ('http', 'https') or not url.hostname or url.username or url.password or url.query or url.fragment:
        raise ValueError('Use an HTTP(S) node RPC URL without embedded credentials')
    if not config.get('rpc_user') or ':' in config['rpc_user'] or '\n' in config['rpc_user']:
        raise ValueError('Invalid RPC username')
    if not config.get('rpc_password') or '\n' in config['rpc_password']:
        raise ValueError('Invalid RPC password')
    return config


def configure(root=ROOT):
    os.umask(0o077)
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    if root.is_symlink() or (root / 'backend.json').exists():
        raise ValueError('Configuration already exists; stop the app before an explicit backend change')
    config = validate({'network': input('Network (main/regtest): ').strip(),
                       'rpc_url': input('Private XBT node RPC URL: ').strip(),
                       'rpc_user': input('RPC username: ').strip(),
                       'rpc_password': getpass.getpass('RPC password: ')})
    # Validate the chain before saving credentials or starting an index.
    from pruned_rpc import Node
    cookie = root / 'backend.cookie'
    cookie.write_text(config['rpc_user'] + ':' + config['rpc_password'])
    cookie.chmod(0o600)
    try:
        node = Node(config['rpc_url'], cookie)
        chain = node.call('getblockchaininfo')
        if chain['chain'] != config['network']:
            raise ValueError('Backend network differs from selected network')
        if config['network'] == 'main' and len(node.call('getblockheader', [chain['bestblockhash'], False])) != 328:
            raise ValueError('Activated Bitcoin Blake2b node required; SHA-256 BTC is incompatible')
        (root / 'client.cookie').write_text('paperclip:' + secrets.token_urlsafe(48))
        (root / 'backend.json').write_text(json.dumps({'network': config['network'], 'rpc_url': config['rpc_url']}))
    except Exception:
        cookie.unlink(missing_ok=True)
        raise
    print('Private adapter configured. It starts with the wallet service. Wait for indexing before wallet creation.')


def connection(root=ROOT):
    user, password = (root / 'client.cookie').read_text().strip().split(':', 1)
    print(json.dumps({'rpc_url': 'http://127.0.0.1:18336', 'rpc_user': user,
                      'rpc_password': password, 'private': True}))


def status(root=ROOT):
    from pruned_rpc import Node
    print(json.dumps(Node('http://127.0.0.1:18336', root / 'client.cookie').call('getpaperclipindexinfo')))


def adapter_args(root=ROOT, lib=LIB):
    config = json.loads((root / 'backend.json').read_text())
    if config['network'] not in ('main', 'regtest'):
        raise ValueError('Invalid adapter network')
    args = ['python3', str(lib / 'pruned_rpc.py'), '--node-url', config['rpc_url'],
            '--node-cookie', str(root / 'backend.cookie'), '--client-cookie', str(root / 'client.cookie'),
            '--database', str(root / 'index.sqlite'), '--first-height', '0' if config['network'] == 'regtest' else '961640']
    if config['network'] == 'regtest':
        args.append('--regtest')
    return args


def supervise():
    os.umask(0o077)
    # Allows the explicit adapter handshake. Native txindex remains supported.
    os.environ['PAPERCLIP_PRUNED_RPC'] = '1'
    stopped = False
    def stop(*_):
        nonlocal stopped
        stopped = True
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    adapter = None
    # Start the adapter before a saved wallet attempts its backend handshake.
    if (ROOT / 'backend.json').exists():
        adapter = subprocess.Popen(adapter_args(), stdin=subprocess.DEVNULL)
        time.sleep(1)
    wallet = None
    retry_at = 0
    try:
        while not stopped and (wallet is None or wallet.poll() is None):
            if (ROOT / 'backend.json').exists() and (adapter is None or adapter.poll() is not None) and time.monotonic() >= retry_at:
                adapter = subprocess.Popen(adapter_args(), stdin=subprocess.DEVNULL)
                retry_at = time.monotonic() + 10
            if wallet is None:
                config_path = Path(os.environ.get('BARKD_DATADIR', '/data/wallet')) / 'config.toml'
                config = tomllib.loads(config_path.read_text()) if config_path.exists() else {}
                uses_adapter = (config.get('bitcoind_address') or '').rstrip('/') == 'http://127.0.0.1:18336'
                ready = not uses_adapter
                if uses_adapter:
                    try:
                        from pruned_rpc import Node
                        ready = Node('http://127.0.0.1:18336', ROOT / 'client.cookie').call('getpaperclipindexinfo')['synced']
                    except Exception:
                        ready = False
                if ready:
                    wallet = subprocess.Popen(['python3', str(LIB / 'entrypoint.py')], stdin=subprocess.DEVNULL)
                else:
                    time.sleep(2)
            time.sleep(.5)
    finally:
        for child in (wallet, adapter):
            if child and child.poll() is None:
                child.terminate()
        for child in (wallet, adapter):
            if child:
                try:
                    child.wait(timeout=90)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
    return 0 if stopped else (wallet.returncode if wallet else 1) or 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['run', 'configure', 'connection', 'status'], nargs='?', default='run')
    command = parser.parse_args().command
    try:
        if command == 'configure': configure()
        elif command == 'connection': connection()
        elif command == 'status': status()
        else: sys.exit(supervise())
    except Exception:
        sys.exit('Private chain adapter unavailable. Check node access, network, and saved configuration. No wallet keys were changed.')
