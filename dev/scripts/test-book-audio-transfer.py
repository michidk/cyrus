#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('transfer',Path(__file__).with_name('book-audio-transfer.py'))
transfer=importlib.util.module_from_spec(spec);spec.loader.exec_module(transfer)

class Transfer(unittest.TestCase):
    def report(self,gain=1,success=True,digest='same'):
        return dict(success=success,references=[dict(sha256=digest,median_gain=gain)])

    def test_measures_executed_response_instead_of_assuming_unity(self):
        result=transfer.compare(self.report(.5),self.report(.4))
        self.assertAlmostEqual(result[0]['gain_ratio'],.8)

    def test_rejects_unadmitted_changed_or_missing_source(self):
        for before,after in [(self.report(success=False),self.report()),
            (self.report(),self.report(success=False)),(self.report(),self.report(digest='changed')),
            (self.report(0),self.report())]:
            with self.assertRaises(ValueError):transfer.compare(before,after)

if __name__=='__main__':unittest.main()
