#!/usr/bin/env python3
"""Isolated Linux smoke test against built paperclip-walletd; no node or funds used."""
import base64
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='paperclip-platform-') as temporary:
    data = Path(temporary) / 'wallet'
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    secret = secrets.token_hex(32)
    token = base64.urlsafe_b64encode(b'\0' + bytes.fromhex(secret)).decode()
    env = dict(os.environ, BARKD_DATADIR=str(data), BARKD_BIND_HOST='127.0.0.1',
               BARKD_BIND_PORT=str(port), APP_PASSWORD=secret)
    url = f'http://127.0.0.1:{port}'

    def request(path, authenticated=True, body=None):
        headers = {'Content-Type': 'application/json'}
        if authenticated:
            headers['Authorization'] = 'Bearer ' + token
        req = Request(url + path, headers=headers, data=None if body is None else json.dumps(body).encode())
        with urlopen(req, timeout=5) as response:
            return response.read()

    with (Path(temporary) / 'daemon.log').open('w') as log:
        process = subprocess.Popen(['python3', str(root / 'deployment/container-entrypoint.py')],
                                   env=env, stdout=log, stderr=log)
        try:
            for attempt in range(60):
                if process.poll() is not None:
                    raise AssertionError('Daemon exited before readiness; test logs remain private')
                try:
                    state = json.loads(request('/api/v1/wallet'))
                    break
                except HTTPError as error:
                    raise AssertionError(f'Setup status endpoint returned HTTP {error.code}') from None
                except URLError:
                    time.sleep(0.25)
            else:
                raise AssertionError('Daemon never became ready')
            assert state == {'fingerprint': None}, state
            try:
                request('/api/v1/wallet', authenticated=False)
                raise AssertionError('Unauthenticated wallet API was accessible')
            except HTTPError as error:
                assert error.code == 401, error.code
            assert b'id="create-wallet"' in request('/', authenticated=False)
            assert b"api('wallet')" in request('/app.js', authenticated=False)
            # Reject setup against a nonexistent local backend. No live RPC calls.
            try:
                request('/api/v1/wallet/create', body={
                    'network': 'regtest', 'ark_server': 'http://127.0.0.1:1', 'force': False,
                    'chain_source': {'bitcoind': {'bitcoind': 'http://127.0.0.1:1',
                        'bitcoind_auth': {'user-pass': {'user': 'isolated', 'pass': 'isolated'}}}}
                })
                raise AssertionError('Invalid setup unexpectedly succeeded')
            except HTTPError as error:
                assert 400 <= error.code < 600
            assert json.loads(request('/api/v1/wallet')) == {'fingerprint': None}
            assert (data / 'auth_token').read_text() == token
            assert (data / 'auth_token').stat().st_mode & 0o777 == 0o600
            assert not (data / 'mnemonic').exists(), 'Failed setup left key material behind'
        finally:
            process.terminate()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
print('PASS: isolated daemon, authenticated setup, embedded UI, invalid-backend rejection, token preservation, and file permissions')
