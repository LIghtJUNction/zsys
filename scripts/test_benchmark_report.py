import unittest
from benchmark_report import metrics

class RawMetrics(unittest.TestCase):
    def test_known_distribution(self):
        rows = [dict(ns=n, units=2, failed=0) for n in range(1,101)]
        result = metrics(rows)
        self.assertEqual(result['mean_ns'],50.5)
        self.assertEqual(result['median_ns'],50.5)
        self.assertEqual(result['p95_ns'],95)
        self.assertEqual(result['p99_ns'],99)
        self.assertEqual(result['max_ns'],100)
        self.assertEqual(result['operations_per_second'],1e9/50.5)
    def test_failures_and_invalid_samples_are_not_dropped(self):
        result = metrics([dict(ns=3,units=1,failed=1),dict(ns=7,units=2,failed=0)])
        self.assertEqual(result['failures'],1)
        self.assertEqual((result['units_min'],result['units_max']),(1,2))
        with self.assertRaises(ValueError): metrics([])
        with self.assertRaises(ValueError): metrics([dict(ns=0,units=1,failed=0)])
        with self.assertRaises(ValueError): metrics([dict(ns=3,units=0,failed=0)])

if __name__ == '__main__': unittest.main()
