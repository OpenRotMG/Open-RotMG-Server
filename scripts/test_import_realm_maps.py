import importlib.util
from pathlib import Path
import struct
import unittest
import zlib

spec = importlib.util.spec_from_file_location('realm_import', Path(__file__).with_name('import-realm-maps.py'))
realm_import = importlib.util.module_from_spec(spec)
spec.loader.exec_module(realm_import)


class ElevationImportTests(unittest.TestCase):
    def test_v2_source_height_is_third_byte_in_row_major_order(self):
        header = struct.pack('<hh', 1, 24) + b'\0\0\1\0' + struct.pack('<ii', 2048, 2048)
        payload = bytearray(2048 * 2048 * 3)
        for x, y, height in [(0, 0, 0), (592, 137, 4), (137, 592, 123), (2047, 2047, 255)]:
            payload[(y * 2048 + x) * 3 + 2] = height
        raw = b'\x02' + zlib.compress(header + payload)
        actual_header, dictionary, cells = realm_import.source_cells(raw)
        self.assertEqual(actual_header, header)
        self.assertEqual(dictionary, [(24, '', '', 1, 0)])
        self.assertEqual(cells[2::3][137 * 2048 + 592], 4)
        self.assertEqual(cells[2::3][592 * 2048 + 137], 123)
        self.assertEqual(cells[2::3][-1], 255)
        for invalid in [raw[1:], raw + b'trailing', b'\x02' + zlib.compress(header + payload[:-1]),
                        b'\x02' + zlib.compress(header + payload + b'extra')]:
            with self.subTest(length=len(invalid)), self.assertRaises(ValueError):
                realm_import.source_cells(invalid)


if __name__ == '__main__':
    unittest.main()
