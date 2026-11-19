import unittest

from calc import add, divide


class CalcTest(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(1, 2), 3)

    def test_divide(self):
        self.assertEqual(divide(6, 3), 2)


if __name__ == "__main__":
    unittest.main()
