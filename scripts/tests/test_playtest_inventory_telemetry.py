import unittest

from scripts.playtest import inventory_payload_totals, process_memory


class InventoryTelemetryTests(unittest.TestCase):
    def test_reads_resident_memory_not_virtual_size(self):
        self.assertEqual(
            process_memory("VmSize:\t99000 kB\nVmHWM:\t2500 kB\nVmRSS:\t2300 kB\n"),
            {"rss_kib": 2300, "hwm_kib": 2500},
        )
        self.assertIsNone(process_memory("Name:\tclient\nState:\tZ (zombie)\n"))

    def test_payload_totals_exclude_actor_snapshots_and_do_not_sum_cumulative_rows(self):
        log = (
            "TK PERF snapshot_bytes=3200 actors=1\n"
            "TK INVENTORY PAYLOAD request_bytes=0 request_messages=0 update_bytes=36 update_messages=1\n"
            "TK INVENTORY PAYLOAD request_bytes=16 request_messages=1 update_bytes=97 update_messages=3\n"
        )
        self.assertEqual(inventory_payload_totals(log), {
            "request_bytes": 16, "request_messages": 1,
            "update_bytes": 97, "update_messages": 3,
        })
        with self.assertRaisesRegex(AssertionError, "Missing server inventory"):
            inventory_payload_totals("TK PERF snapshot_bytes=3200 actors=1")
