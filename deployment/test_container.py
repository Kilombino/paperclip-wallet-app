import base64
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('entrypoint', Path(__file__).with_name('container-entrypoint.py'))
entrypoint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(entrypoint)


class BootstrapTest(unittest.TestCase):
    def test_password_is_auth_only_and_restart_preserves_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            data = Path(directory) / 'wallet'
            password = '12' * 32
            entrypoint.prepare_auth(data, password)
            self.assertEqual(set(p.name for p in data.iterdir()), {'auth_token'})
            token = (data / 'auth_token').read_text()
            self.assertEqual(base64.urlsafe_b64decode(token), b'\0' + bytes.fromhex(password))
            (data / 'mnemonic').write_text('existing-key-material')
            entrypoint.prepare_auth(data, password)
            self.assertEqual((data / 'mnemonic').read_text(), 'existing-key-material')
            with self.assertRaises(ValueError):
                entrypoint.prepare_auth(data, '34' * 32)
            self.assertEqual((data / 'auth_token').read_text(), token)
            entrypoint.prepare_auth(data)

    def test_native_auth_is_left_to_daemon(self):
        with tempfile.TemporaryDirectory() as directory:
            entrypoint.prepare_auth(directory)
            self.assertEqual(list(Path(directory).iterdir()), [])
            with self.assertRaises(ValueError):
                entrypoint.prepare_auth(directory, 'weak-password')

    def test_corrupt_backup_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / 'auth_token').write_text('invalid')
            with self.assertRaises(ValueError):
                entrypoint.prepare_auth(directory)


if __name__ == '__main__':
    unittest.main()
