import unittest
from run import percentile, summarize

class Statistics(unittest.TestCase):
    def test_nearest_rank(self):
        self.assertEqual(percentile(list(range(1, 101)), .99), 99)
        self.assertEqual(percentile([7], .95), 7)
        with self.assertRaises(ValueError):
            percentile([], .99)
    def test_uninstrumented_does_not_claim_zero_allocations(self):
        rows = [dict(ns='10', units='1', failed='0', allocations='0', allocated_bytes='0', peak_extra_live_bytes='0')]
        s = summarize(rows, 'VmHWM: 25 kB', False)
        self.assertNotIn('allocations_per_op', s)
        self.assertEqual(s['process_peak_rss_bytes'], 25*1024)
        self.assertEqual(s['operations_per_second'], 100000000)
    def test_invalid_empty_work(self):
        with self.assertRaises(ValueError):
            summarize([dict(ns='5', units='0', failed='0')], '', False)

if __name__ == '__main__':
    unittest.main()
