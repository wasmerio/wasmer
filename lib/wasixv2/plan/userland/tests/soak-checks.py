#!/usr/bin/env python3
"""Bounded controls for the source-only soak observer."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("soak", Path(__file__).with_name("soak.py"))
soak = importlib.util.module_from_spec(spec)
spec.loader.exec_module(soak)


class GrowthFlags(unittest.TestCase):
    def flagged(self, blocks):
        rows = [dict(age_s=300+60*i, rss_kib=value)
                for i, value in enumerate(v for b in blocks for v in [b]*5)]
        return soak.flags(rows, "rss_kib", 32768)

    def test_late_monotonic_growth_after_an_early_decrease(self):
        self.assertTrue(self.flagged([120000, 90000, 100000, 140000]))

    def test_flat_or_decreasing_or_small_growth(self):
        for blocks in [[90000]*4, [140000, 100000, 90000], [90000, 100000, 110000]]:
            with self.subTest(blocks=blocks):
                self.assertFalse(self.flagged(blocks))

    def test_needs_three_consecutive_nondecreasing_blocks(self):
        self.assertFalse(self.flagged([90000, 140000, 80000, 130000]))

    def test_warmup_does_not_flag(self):
        rows = [dict(age_s=i, rss_kib=i*100000) for i in range(20)]
        self.assertFalse(soak.flags(rows, "rss_kib", 32768))


if __name__ == "__main__":
    unittest.main()
