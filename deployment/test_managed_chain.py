import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('managed', Path(__file__).with_name('managed-chain.py'))
managed = importlib.util.module_from_spec(spec)
spec.loader.exec_module(managed)


class ManagedChainTests(unittest.TestCase):
    def test_private_arguments_do_not_expose_rpc_password_or_allow_short_history(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'backend.json').write_text(json.dumps({'network': 'main', 'rpc_url': 'http://node:8332'}))
            args = managed.adapter_args(root)
            self.assertEqual(args[args.index('--first-height') + 1], '961640')
            self.assertNotIn('--bind', args)
            self.assertIn(str(root / 'backend.cookie'), args)
            (root / 'backend.json').write_text(json.dumps({'network': 'regtest', 'rpc_url': 'http://node:18443'}))
            args = managed.adapter_args(root)
            self.assertIn('--regtest', args)
            self.assertEqual(args[args.index('--first-height') + 1], '0')

    def test_reject_credentials_in_url_and_invalid_network(self):
        base = {'network': 'main', 'rpc_url': 'http://node:8332', 'rpc_user': 'owner', 'rpc_password': 'test-password'}
        managed.validate(base)
        for update in ({'rpc_url': 'http://user:secret@node/'}, {'network': 'signet'}, {'rpc_user': 'owner:secret'}, {'rpc_url': 'file:///etc/passwd'}):
            with self.assertRaises(ValueError):
                managed.validate(dict(base, **update))
