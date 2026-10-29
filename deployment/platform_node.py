"""Resolve a package-supplied node without exposing credentials to the browser."""
import json
import os
import secrets
import tempfile
from pathlib import Path
from urllib.parse import urlsplit


def atomic_write(path, value):
    path = Path(path)
    if path.is_symlink():
        raise ValueError('Refusing a symbolic link')
    fd, temporary = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as out:
            out.write(value)
            out.flush()
            os.fsync(out.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def prepare(root, env, node_factory):
    url = env.get('PAPERCLIP_NODE_RPC_URL', '')
    if not url:
        return False
    parsed = urlsplit(url)
    if parsed.scheme not in ('http', 'https') or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise ValueError('Invalid installed node RPC URL')
    root = Path(root)
    if root.is_symlink():
        raise ValueError('Refusing a symbolic link')
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    binding = root / 'platform-source.json'
    if binding.exists() and json.loads(binding.read_text()) != {'rpc_url': url}:
        raise ValueError('Saved wallet node changed; migrate its connection explicitly')
    cookie = env.get('PAPERCLIP_NODE_RPC_COOKIE')
    if not cookie:
        user, password = env.get('PAPERCLIP_NODE_RPC_USER', ''), env.get('PAPERCLIP_NODE_RPC_PASSWORD', '')
        if not user or not password or ':' in user or any(c in user + password for c in '\r\n'):
            raise ValueError('Installed node credentials are incomplete')
        cookie = root / 'platform.cookie'
        atomic_write(cookie, user + ':' + password)
    node = node_factory(url, cookie)
    chain = node.call('getblockchaininfo')
    if chain.get('chain') != 'main' or chain.get('initialblockdownload', True):
        raise ValueError('Wait for the installed XBT mainnet node to synchronize')
    header = node.call('getblockheader', [chain['bestblockhash'], False])
    if not isinstance(header, str) or len(header) != 328:
        raise ValueError('Installed node is not an activated Blake2b node')
    indexes = node.call('getindexinfo')
    if not indexes.get('txindex', {}).get('synced'):
        backend = root / 'backend.json'
        expected = {'network': 'main', 'rpc_url': url, 'node_cookie': str(cookie), 'platform_managed': True}
        if backend.exists() and json.loads(backend.read_text()) != expected:
            raise ValueError('Saved adapter uses another connection; change it explicitly')
        client_cookie = root / 'client.cookie'
        if not client_cookie.exists():
            atomic_write(client_cookie, 'paperclip:' + secrets.token_urlsafe(48))
        if not backend.exists():
            atomic_write(backend, json.dumps(expected))
        url, cookie = 'http://127.0.0.1:18336', client_cookie
    connection = {'bitcoind': {'bitcoind': url, 'bitcoind_auth': {'cookie': {'cookie': str(cookie)}}}}
    if not binding.exists():
        atomic_write(binding, json.dumps({'rpc_url': env['PAPERCLIP_NODE_RPC_URL']}))
    atomic_write(root / 'platform-wallet.json', json.dumps(connection))
    return True
