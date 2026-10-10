"""Fake sysfs tests; never write to host device settings."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('usb_wake', Path(__file__).resolve().parents[1] / 'installer/genkan/rust/system/usb-wake.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Wake(unittest.TestCase):
    def test_usb_input_enables_only_its_ancestors_and_survives_reenumeration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            controller = root / 'devices/pci/controller'
            hub = controller / 'usb3/3-5'
            keyboard = hub / '3-5.3'
            event = keyboard / 'interface/input/input20/event20'
            event.mkdir(parents=True)
            camera = controller / 'usb3/3-4'
            for device in [controller, controller / 'usb3', hub, keyboard, camera]:
                (device / 'power').mkdir(parents=True)
                (device / 'power/wakeup').write_text('disabled\n')
                (device / 'power/control').write_text('auto\n')
            (keyboard / 'idVendor').touch()
            (keyboard / 'idProduct').touch()
            self.assertEqual(len(module.enable(event, root)), 4)
            self.assertEqual((camera / 'power/wakeup').read_text(), 'disabled\n')
            self.assertEqual((hub / 'power/control').read_text(), 'auto\n')
            self.assertEqual(module.enable(event, root), [])
            (hub / 'power/wakeup').write_text('disabled\n')
            self.assertEqual(module.enable(event, root), [hub / 'power/wakeup'])

    def test_non_usb_and_paths_outside_devices_are_untouched(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            device = root / 'devices/platform/input'
            (device / 'power').mkdir(parents=True)
            (device / 'power/wakeup').write_text('disabled\n')
            self.assertEqual(module.enable(device, root), [])
            self.assertEqual((device / 'power/wakeup').read_text(), 'disabled\n')
            with self.assertRaises(ValueError):
                module.enable(root, root)


if __name__ == '__main__':
    unittest.main()
