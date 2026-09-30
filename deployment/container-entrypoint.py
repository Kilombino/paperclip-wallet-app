#!/usr/bin/env python3
"""Container bootstrap. Authentication only; wallet keys are created by barkd."""
import base64
import hmac
import os
from pathlib import Path
import re
import sys


def prepare_auth(datadir, app_password=None):
    datadir = Path(datadir)
    datadir.mkdir(mode=0o700, parents=True, exist_ok=True)
    if datadir.is_symlink():
        raise ValueError('Wallet directory must not be a symbolic link')
    os.chmod(datadir, 0o700)
    path = datadir / 'auth_token'
    if path.is_symlink():
        raise ValueError('Authentication file must not be a symbolic link')
    expected = None
    if app_password:
        if not re.fullmatch(r'[0-9a-fA-F]{64}', app_password):
            raise ValueError('APP_PASSWORD must contain 32 random bytes encoded as hex')
        expected = base64.urlsafe_b64encode(b'\0' + bytes.fromhex(app_password)).decode().rstrip('=')
    if path.exists():
        current = path.read_text()
        try:
            decoded = base64.b64decode(current, altchars=b'-_', validate=True)
        except ValueError:
            raise ValueError('Invalid persisted authentication token') from None
        if len(decoded) != 33 or decoded[0] != 0:
            raise ValueError('Invalid persisted authentication token')
        if expected and not hmac.compare_digest(current, expected):
            raise ValueError('App password differs from restored token; resolve explicitly before startup')
        os.chmod(path, 0o600)
    elif expected:
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as handle:
            handle.write(expected)
            handle.flush()
            os.fsync(handle.fileno())
    # Without a platform password, barkd generates its native random token.
    # Never create, replace, or derive a mnemonic here.


def main():
    os.umask(0o077)
    datadir = os.environ.get('BARKD_DATADIR', '/data/wallet')
    prepare_auth(datadir, os.environ.pop('APP_PASSWORD', None))
    os.environ.pop('APP_SEED', None)
    os.environ['BARKD_DATADIR'] = datadir
    os.environ.setdefault('BARKD_BIND_HOST', '0.0.0.0')
    os.environ.setdefault('BARKD_BIND_PORT', '3000')
    for unsafe in ('BARKD_NO_AUTH', 'BARKD_DANGEROUSLY_ALLOW_REMOTE_NO_AUTH', 'BARKD_EXPOSE_MNEMONIC'):
        os.environ.pop(unsafe, None)
    os.execvp('paperclip-walletd', ['paperclip-walletd'])


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError):
        sys.exit('Wallet bootstrap failed. Check volume permissions and app password/backup consistency; no keys were replaced.')
