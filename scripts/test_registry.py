import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import registry

class Registry(unittest.TestCase):
    def test_missing_matching_and_conflicting_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory)/'crate'; archive.write_bytes(b'test archive')
            with patch('registry.record',return_value=None):
                self.assertFalse(registry.check('0.1.0',archive))
            with patch('registry.record',return_value={'checksum':hashlib.sha256(archive.read_bytes()).hexdigest()}):
                self.assertTrue(registry.check('0.1.0',archive))
            with patch('registry.record',return_value={'checksum':'different'}):
                with self.assertRaises(RuntimeError): registry.check('0.1.0',archive)

if __name__ == '__main__': unittest.main()
