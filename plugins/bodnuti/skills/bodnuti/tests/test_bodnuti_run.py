#!/usr/bin/env python3
"""Offline tests for bodnuti_run.py. They run no container and no git.

Run from the plugin directory:
    python3 -m unittest discover -s skills/bodnuti/tests
"""

import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / 'scripts'))

import bodnuti_run  # noqa: E402


class EngineTests(unittest.TestCase):
    def test_prefers_podman(self):
        with mock.patch('shutil.which', side_effect=lambda n: f'/usr/bin/{n}'):
            self.assertEqual(bodnuti_run.pick_engine(), 'podman')

    def test_falls_back_to_docker(self):
        with mock.patch('shutil.which', side_effect=lambda n: '/bin/docker' if n == 'docker' else None):
            self.assertEqual(bodnuti_run.pick_engine(), 'docker')

    def test_none_installed(self):
        with mock.patch('shutil.which', return_value=None), self.assertRaises(SystemExit):
            bodnuti_run.pick_engine()


class RangeBaseTests(unittest.TestCase):
    def test_two_dot_range(self):
        self.assertEqual(bodnuti_run.range_base('origin/master..HEAD'), 'origin/master')

    def test_single_rev(self):
        self.assertEqual(bodnuti_run.range_base('HEAD~3'), 'HEAD~3')


class CommandTests(unittest.TestCase):
    def test_podman_keeps_uid_and_relabels(self):
        cmd = bodnuti_run.container_command('podman', 'img:latest', '/w', ['--repo', '/work/repo'],
                                            {'BODNUTI_OECORE': '/work/oecore'}, 'newer')
        self.assertEqual(cmd[:4], ['podman', 'run', '--rm', '--pull=newer'])
        self.assertIn('--userns=keep-id', cmd)
        self.assertIn('/w:/work:Z', cmd)
        self.assertIn('BODNUTI_OECORE=/work/oecore', cmd)
        self.assertEqual(cmd[-4:], ['img:latest', 'check', '--repo', '/work/repo'])

    def test_docker_runs_as_user(self):
        cmd = bodnuti_run.container_command('docker', 'img:0.3.0', '/w', [])
        self.assertIn('--user', cmd)
        self.assertIn('/w:/work', cmd)
        self.assertNotIn('--userns=keep-id', cmd)
        self.assertIn('--pull=missing', cmd)


if __name__ == '__main__':
    unittest.main()
