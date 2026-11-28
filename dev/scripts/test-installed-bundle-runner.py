#!/usr/bin/env python3
"""Installed bundle projections bind original source, raw files and durable results."""
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
import zipfile
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('installed-bundle-runner.py'))
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)


class Tests(unittest.TestCase):
    def test_failed_and_interrupted_tests_retain_source_and_fixture_before_execution(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);log=root/'test.log';attempt=root/'attempt.json'
            binding=dict(source='immutable',fixture='authentic',image='pinned')
            def fail(command, **kwargs):
                self.assertEqual(json.loads(attempt.read_text())['binding'],binding)
                self.assertEqual(json.loads(attempt.read_text())['stage'],'running')
                process=unittest.mock.MagicMock()
                process.__enter__.return_value=process
                process.stdout=iter(['actual ownership rejection\n'])
                process.wait.return_value=101
                return process
            with patch.object(m.subprocess,'Popen',side_effect=fail):
                self.assertEqual(m.recorded_test(['test'],log,attempt,binding).returncode,101)
            record=json.loads(attempt.read_text())
            self.assertEqual(record['exit'],101);self.assertEqual(record['log_sha256'],m.sha(log))
            self.assertEqual(record['binding'],binding)
            with patch.object(m.subprocess,'Popen',side_effect=OSError('execution lost')):
                with self.assertRaises(OSError):m.recorded_test(['test'],log,attempt,binding)
            self.assertEqual(json.loads(attempt.read_text())['stage'],'running')
            self.assertEqual(json.loads(attempt.read_text())['binding'],binding)

    def test_runtime_recovery_uses_digest_bound_archive_and_pinned_image(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d); archive=root/'runtime-image.tar.gz';archive.write_bytes(b'archive')
            descriptor=dict(image='sha256:pinned',bytes=7,sha256=m.sha(archive))
            (root/'runtime-image.json').write_text(json.dumps(descriptor))
            missing=unittest.mock.Mock(returncode=1,stdout='')
            ready=unittest.mock.Mock(returncode=0,stdout='sha256:pinned\n')
            with patch.object(m.subprocess,'run',side_effect=[missing,unittest.mock.Mock(returncode=0),ready]) as run:
                self.assertEqual(m.runtime_image(root,'task-runtime'),'sha256:pinned')
                self.assertNotIn('--retain',run.call_args_list[1].args[0])
                self.assertIn('sha256:pinned',run.call_args_list[1].args[0])
            with patch.object(m.subprocess,'run',return_value=ready) as run:
                self.assertEqual(m.runtime_image(root,'task-runtime'),'sha256:pinned')
                self.assertEqual(run.call_count,1)
            with patch.object(m.subprocess,'run',return_value=unittest.mock.Mock(returncode=0,stdout='sha256:wrong')):
                with self.assertRaisesRegex(ValueError,'pinned retained identity'):m.runtime_image(root,'task-runtime')
            archive.write_bytes(b'changed')
            with patch.object(m.subprocess,'run',return_value=missing) as run:
                with self.assertRaisesRegex(ValueError,'digest or size'):m.runtime_image(root,'task-runtime')
                self.assertEqual(run.call_count,1)
            (root/'runtime-image.json').unlink()
            with patch.object(m.subprocess,'run',return_value=missing):
                with self.assertRaisesRegex(ValueError,'no retained runtime descriptor'):m.runtime_image(root,'task-runtime')

    def test_original_save_install_uses_read_only_group_without_changing_install(self):
        with tempfile.TemporaryDirectory() as d:
            saves = Path(d) / 'SAVEGAME'
            (saves / 'SAVEGAME.000').mkdir(parents=True)
            args = m.original_save_install_arguments(saves)
            self.assertEqual(args, ['-v', f'{saves}:/fixture-install/SAVEGAME:ro',
                                    '-e', 'REDGUARD_ORIGINAL_SAVE_INSTALL=/fixture-install'])
            self.assertEqual(sorted(path.name for path in Path(d).iterdir()), ['SAVEGAME'])

    def test_libtest_worker_limit_preserves_full_suite_and_default_arguments(self):
        self.assertEqual(m.libtest_arguments(None), ['--nocapture'])
        for workers in [1, 4]:
            self.assertEqual(m.libtest_arguments(workers),
                             ['--nocapture', f'--test-threads={workers}'])
        for workers in [0, -1]:
            with self.assertRaisesRegex(ValueError, 'positive'):
                m.libtest_arguments(workers)

    def test_single_target_retry_requires_package_and_exact_manifest_harness(self):
        manifest = dict(files=[dict(role='test', package='script', target='original_save'),
                               dict(role='test', package='other', target='original_save'),
                               dict(role='companion', package='script', target='cyrus')])
        self.assertEqual(m.selected_tests(manifest, 'script', 'original_save'), manifest['files'][:1])
        self.assertEqual(len(m.selected_tests(manifest, None, None)), 2)
        with self.assertRaisesRegex(ValueError, 'requires a package'):
            m.selected_tests(manifest, None, 'original_save')
        for target in ['missing', 'cyrus']:
            with self.assertRaisesRegex(ValueError, 'exactly one exported test'):
                m.selected_tests(manifest, 'script', target)
        with self.assertRaisesRegex(ValueError, 'exactly one exported test'):
            m.selected_tests(dict(files=manifest['files'] * 2), 'script', 'original_save')

    def test_exact_source_rejects_modified_missing_or_extra_blobs(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);path=root/'source.rs';path.write_bytes(b'current source')
            object_id=hashlib.sha1(b'blob 14\0current source').hexdigest()
            records=f'100644 blob {object_id}\tsource.rs'.encode()+b'\0'
            m.verify_source(root,records)
            path.write_bytes(b'changed source')
            with self.assertRaisesRegex(ValueError,'blob differs'):m.verify_source(root,records)
            path.write_bytes(b'current source');(root/'extra').write_text('extra')
            with self.assertRaisesRegex(ValueError,'census'):m.verify_source(root,records)
            path.unlink()
            with self.assertRaisesRegex(ValueError,'absent'):m.verify_source(root,records)

    def test_read_only_parent_has_prepared_nested_bind_targets(self):
        with tempfile.TemporaryDirectory() as d:
            source=Path(d)
            m.verify_source(source,b'')
            m.prepare_mountpoints(source)
            self.assertTrue(all((source/name).is_dir() for name in ['redguard','target','magic','game-config']))
            m.verify_source(source,b'')
            (source/'game-config'/'leaked-state').write_text('not immutable source')
            with self.assertRaisesRegex(ValueError,'census'):m.verify_source(source,b'')

    def test_cd_media_plan_removes_empty_gog_placeholders_before_source_mount(self):
        with tempfile.TemporaryDirectory() as d:
            source = Path(d)
            for name in ['game.gog', 'game.ins']:
                (source / name).touch()
            m.prepare_mountpoints(source)
            self.assertFalse((source / 'game.gog').exists())
            self.assertFalse((source / 'game.ins').exists())
            m.verify_source(source, b'')

    def test_media_plan_preserves_current_and_tracked_placeholders(self):
        with tempfile.TemporaryDirectory() as d:
            source = Path(d)
            (source / 'game.gog').touch()
            (source / 'game.ins').touch()
            m.prepare_mountpoints(source, empty_files=['game.gog'])
            self.assertTrue((source / 'game.gog').is_file())
            self.assertFalse((source / 'game.ins').exists())
            records = b'100644 blob ' + hashlib.sha1(b'blob 0\0').hexdigest().encode() + b'\tgame.gog\0'
            m.prepare_mountpoints(source, records)
            self.assertTrue((source / 'game.gog').is_file())
            m.verify_source(source, records)

    def test_media_plan_rejects_real_files_and_symlinks_before_removing_anything(self):
        for kind in ['file', 'symlink', 'broken-symlink']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as d:
                source = Path(d) / 'source'
                source.mkdir()
                (source / 'game.gog').touch()
                media = Path(d) / 'media'
                if kind != 'broken-symlink':
                    media.write_bytes(b'owned media')
                if kind != 'file':
                    (source / 'game.ins').symlink_to(media)
                else:
                    (source / 'game.ins').write_bytes(media.read_bytes())
                with self.assertRaisesRegex(ValueError, 'media mountpoint'):
                    m.prepare_mountpoints(source)
                self.assertTrue((source / 'game.gog').is_file())
                if kind != 'broken-symlink':
                    self.assertEqual((source / 'game.ins').read_bytes(), b'owned media')
                else:
                    self.assertTrue((source / 'game.ins').is_symlink())

    def test_daemon_visibility_rejects_locally_valid_but_unshared_source(self):
        with tempfile.TemporaryDirectory() as d:
            source=Path(d);(source/'Cargo.toml').write_text('exact source')
            with patch.object(m.subprocess,'run',return_value=unittest.mock.Mock(returncode=0,stdout=m.sha(source/'Cargo.toml')+'  /source/Cargo.toml')):
                m.verify_daemon_source('pinned-image',source)
            for code,output in [(1,'file missing'),(0,'different  /source/Cargo.toml')]:
                with patch.object(m.subprocess,'run',return_value=unittest.mock.Mock(returncode=code,stdout=output)):
                    with self.assertRaisesRegex(ValueError,'daemon-shared'):m.verify_daemon_source('pinned-image',source)

    def test_fixture_group_is_explicit_and_scoped_to_one_exported_target(self):
        with tempfile.TemporaryDirectory() as d:
            group=Path(d);(group/'SAVEGAME.002').mkdir()
            manifest=dict(files=[dict(role='test',package='script',target='original_save')])
            value=f'script:original_save={group}'
            self.assertEqual(m.fixture_directories(manifest,[value]),{'script:original_save':group.resolve()})
            for invalid in [[value,value],[f'foreign:original_save={group}'],[f'script:original_save={group}/missing']]:
                with self.assertRaises(ValueError):m.fixture_directories(manifest,invalid)
            before=m.fixture_hash(group);(group/'SAVEGAME.002'/'SAVEGAME.SAV').write_bytes(b'fixture')
            self.assertNotEqual(before,m.fixture_hash(group))

    def test_expired_capability_refresh_preserves_exact_range_and_cached_reads(self):
        response=unittest.mock.MagicMock(status=206,headers={'Content-Range':'bytes 0-3/4'})
        response.__enter__.return_value=response
        response.read.return_value=b'abcd'
        expired=m.urllib.error.HTTPError('https://expired',403,'expired',None,None)
        refresh=unittest.mock.Mock(return_value='https://renewed')
        with patch.object(m.urllib.request,'urlopen',side_effect=[expired,response]) as opened:
            remote=m.RemoteZip('https://expired',4,refresh)
            self.assertEqual(remote.read(2),b'ab');self.assertEqual(remote.read(2),b'cd')
            self.assertEqual(opened.call_count,2);refresh.assert_called_once()
        with patch.object(m.urllib.request,'urlopen',side_effect=m.urllib.error.HTTPError('https://missing',404,'missing',None,None)):
            with self.assertRaises(m.urllib.error.HTTPError):m.RemoteZip('https://missing',4,refresh).read(2)
        refresh.assert_called_once()

    def test_transient_range_failures_retry_without_moving_the_cursor(self):
        failures=[m.urllib.error.URLError('temporary DNS failure'),
                  m.urllib.error.HTTPError('https://same',503,'unavailable',None,None)]
        response=unittest.mock.MagicMock(status=206,headers={'Content-Range':'bytes 0-3/4'})
        response.__enter__.return_value=response
        response.read.return_value=b'abcd'
        refresh=unittest.mock.Mock()
        with patch.object(m.urllib.request,'urlopen',side_effect=failures+[response]) as opened, patch.object(m.time,'sleep'):
            remote=m.RemoteZip('https://same',4,refresh)
            self.assertEqual(remote.read(2),b'ab')
            self.assertEqual(remote.read(2),b'cd')
            self.assertEqual(opened.call_count,3)
            self.assertEqual({c.args[0].full_url for c in opened.call_args_list},{'https://same'})
            self.assertEqual({c.args[0].get_header('Range') for c in opened.call_args_list},{'bytes=0-3'})
            refresh.assert_not_called()

    def test_range_retry_is_bounded_and_does_not_accept_bad_or_partial_data(self):
        for error in [m.urllib.error.URLError('DNS'),TimeoutError('timeout'),m.IncompleteRead(b'a',3)]:
            with self.subTest(error=type(error).__name__), patch.object(m.urllib.request,'urlopen',side_effect=error) as opened, patch.object(m.time,'sleep'):
                remote=m.RemoteZip('https://same',4)
                with self.assertRaises(type(error)):remote.read(2)
                self.assertEqual(opened.call_count,3)
                self.assertEqual((remote.pos,remote.cache),(0,b''))
        for status,header,data in [(200,'bytes 0-3/4',b'abcd'),(206,'bytes 1-3/4',b'abcd'),(206,'bytes 0-3/4',b'a')]:
            response=unittest.mock.MagicMock(status=status,headers={'Content-Range':header})
            response.__enter__.return_value=response
            response.read.return_value=data
            with self.subTest(status=status,header=header,data=data), patch.object(m.urllib.request,'urlopen',return_value=response) as opened:
                remote=m.RemoteZip('https://same',4)
                with self.assertRaises(ValueError):remote.read(2)
                self.assertEqual(opened.call_count,1)
                self.assertEqual((remote.pos,remote.cache),(0,b''))

    def test_url_renewal_uses_managed_exact_repository_and_keeps_capability_private(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'private-url'
            response=unittest.mock.Mock(returncode=0,stderr='< Location: https://example.test/capability\n')
            with patch.object(m.subprocess,'run',return_value=response) as command:
                self.assertEqual(m.refresh_artifact_url(17,path),'https://example.test/capability')
                self.assertIn('repos/michidk/cyrus/actions/artifacts/17/zip',command.call_args.args[0])
            self.assertEqual(path.stat().st_mode & 0o777,0o600)
            with patch.object(m.subprocess,'run',return_value=unittest.mock.Mock(returncode=1,stderr='unavailable')):
                with self.assertRaisesRegex(ValueError,'renewal failed'):m.refresh_artifact_url(17,path)

    def test_game_media_mountpoints_are_empty_placeholders_not_packaged_assets(self):
        with tempfile.TemporaryDirectory() as d:
            source=Path(d);media=source/'game.gog';media.touch()
            m.verify_source(source,b'',empty_files=['game.gog'])
            with self.assertRaisesRegex(ValueError,'census'):m.verify_source(source,b'')
            media.write_bytes(b'not allowed game bytes')
            with self.assertRaisesRegex(ValueError,'empty placeholder'):m.verify_source(source,b'',empty_files=['game.gog'])

    def test_new_runtime_receipt_preserves_previous_completed_evidence(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);binding=dict(image='first',fixture='retained')
            first=m.receipt_path(root,'test-hash',binding);first.write_text('old accepted evidence')
            second=m.receipt_path(root,'test-hash',dict(binding,image='second'));second.write_text('new evidence')
            self.assertNotEqual(first,second);self.assertEqual(first.read_text(),'old accepted evidence')
            self.assertEqual(first,m.receipt_path(root,'test-hash',dict(reversed(list(binding.items())))))

    def test_raw_cache_is_hash_checked_once_and_reused_without_decompression(self):
        with tempfile.TemporaryDirectory() as d:
            entry = dict(sha256=hashlib.sha256(b'raw test').hexdigest(), size=8, mode=0o755, payload='payload/0')
            archive = unittest.mock.Mock(); archive.open.return_value = io.BytesIO(gzip.compress(b'raw test'))
            cache = Path(d)/'cache'; first = m.cache_payload(archive, entry, cache)
            second = m.cache_payload(None, entry, cache)
            self.assertEqual(first, second);self.assertEqual(first.read_bytes(), b'raw test')
            archive.open.assert_called_once()
            saved=json.loads((cache/(entry['sha256']+'.json')).read_text());saved['inode']=-1
            (cache/(entry['sha256']+'.json')).write_text(json.dumps(saved))
            self.assertEqual(m.cache_payload(None,entry,cache).read_bytes(),b'raw test')
            first.chmod(0o755);first.write_bytes(b'bad data')
            broken = unittest.mock.Mock(); broken.open.return_value=io.BytesIO(gzip.compress(b'bad data'))
            with self.assertRaisesRegex(ValueError, 'hash mismatch'): m.cache_payload(broken, entry, cache)
            self.assertFalse((cache/(entry['sha256']+'.partial')).exists())

    def test_projection_retains_only_test_runtime_and_required_binary_companions(self):
        test=dict(role='test',target='original_export',path='debug/deps/test',sha256='a')
        companion=dict(role='companion',target='cyrus',path='debug/cyrus',sha256='b')
        runtime=dict(role='runtime',target='rgpre',path='debug/lib.so',sha256='c')
        extra=dict(role='companion',target='movie_window',path='debug/examples/movie_window',sha256='d')
        manifest=dict(files=[test,companion,runtime,extra])
        entries, digest=m.projection(manifest,test)
        self.assertEqual(entries,[test,companion,runtime])
        self.assertNotEqual(digest,m.projection(dict(files=[dict(test,sha256='different'),companion,runtime]),dict(test,sha256='different'))[1])
        with self.assertRaises(ValueError):m.projection(manifest,dict(test,target='foreign'))

    def test_receipt_requires_shared_admission_and_exact_binding(self):
        with tempfile.TemporaryDirectory() as d:
            receipt=Path(d)/'test.json'; log=receipt.with_suffix('.log')
            good='test result: ok. 3 passed; 0 failed; 0 ignored;'
            log.write_text(good)
            loader=Path(d)/'loader.log'; loader.write_text('resolved libraries')
            listed=Path(d)/'list.log'; listed.write_text('three tests')
            entry=dict(role='test',sha256='test')
            manifest=dict(identity=dict(commit='source',tree='tree'),files=[entry])
            binding=dict(manifest='original',projection='new-test',image='pinned',fixture='save',
                         arguments=['--nocapture'])
            admission=dict(accepted=True,commit='source',tree='tree',sha256='test',
                           original_manifest_sha256='original',image='pinned',
                           loader=dict(exit=0,log=str(loader)),list=dict(exit=0,log=str(listed)),
                           tests=dict(exit=0,log=str(log)))
            record=dict(binding=binding,exit=0,install_skips=0,log_sha256=m.sha(log),admission=admission)
            def accepts(): return m.accepted(receipt,binding,manifest,entry)
            self.assertFalse(accepts())
            m.durable(receipt,record);self.assertTrue(accepts())
            foreign=Path(d)/'foreign.log';foreign.write_text(good)
            redirected=dict(record,admission=dict(admission,tests=dict(exit=0,log=str(foreign))))
            m.durable(receipt,redirected);self.assertFalse(accepts())
            m.durable(receipt,record)
            for key in binding:
                self.assertFalse(m.accepted(receipt,dict(binding,**{key:'different'}),manifest,entry))
            for phase in ['loader','list','tests']:
                bad=dict(record,admission=dict(admission,**{phase:dict(exit=1,log=str(log))}))
                m.durable(receipt,bad);self.assertFalse(accepts())
            m.durable(receipt,record)
            loader.write_text('SKIP missing required fixture');self.assertFalse(accepts())
            loader.write_text('resolved libraries')
            log.write_text('SKIP required fixture\n'+good)
            m.durable(receipt,dict(record,log_sha256=m.sha(log)));self.assertFalse(accepts())
            log.write_text(good);m.durable(receipt,record)
            self.assertFalse(m.accepted(receipt,binding,dict(manifest,identity=dict(commit='new',tree='new')),entry))
            legacy=dict(record);legacy.pop('admission');m.durable(receipt,legacy)
            self.assertFalse(accepts());self.assertTrue(receipt.exists())
            self.assertFalse(receipt.with_suffix('.json.partial').exists())

    def test_missing_failed_or_install_skipped_test_results_reject(self):
        good='test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'
        self.assertEqual(m.check_output(0,good),3)
        # Match Cargo's existing treatment of explicitly ignored optional WASM fixtures.
        self.assertEqual(m.check_output(0,good.replace('0 ignored','1 ignored')),3)
        for status,output in [(1,good),(0,''),(0,'skipped: no install\n'+good),(0,'SKIP inventory export: absent\n'+good)]:
            with self.assertRaises(ValueError):m.check_output(status,output)

    def test_durable_result_fsyncs_before_and_after_atomic_publication(self):
        events=[]
        original_sync=m.os.fsync;original_replace=Path.replace
        def synced(fd):events.append('fsync');return original_sync(fd)
        def replaced(path,target):events.append('rename');return original_replace(path,target)
        with tempfile.TemporaryDirectory() as d, patch.object(m.os,'fsync',side_effect=synced) as fsync, patch.object(Path,'replace',replaced):
            result=Path(d)/'result.json';m.durable(result,{'exit':0})
            self.assertEqual(fsync.call_count,2);self.assertEqual(events,['fsync','rename','fsync']);self.assertEqual(json.loads(result.read_text()),{'exit':0})

    def test_metadata_requires_complete_archive_and_original_manifest_hash(self):
        expected=dict(artifact=1,run=2,size=3,sha256='a'*64,commit='b'*40,tree='c'*40)
        archive_identity={k:v for k,v in expected.items() if k not in {'commit','tree'}}
        manifest=dict(version=1,scope='cargo test --locked --workspace --all-features --no-run',files=[],identity=dict(commit=expected['commit'],tree=expected['tree'],run='2',attempt='1',system='Linux',machine='x86_64'))
        raw=json.dumps(manifest).encode()
        provenance=dict(repository='michidk/cyrus',head=expected['commit'],attempt=1,checkout_commit=expected['commit'],checkout_tree=expected['tree'],run=2,packages={'manifest.json':hashlib.sha256(raw).hexdigest()})
        archive=unittest.mock.Mock();archive.read.side_effect=lambda name:raw if name=='manifest.json' else json.dumps(provenance).encode()
        receipt=dict(identity=archive_identity,sha256=expected['sha256'],bytes=3)
        self.assertEqual(m.admit_metadata(archive,expected,receipt)[0],manifest)
        for invalid in [{},dict(receipt,bytes=2),dict(receipt,sha256='d'*64)]:
            with self.assertRaises(ValueError):m.admit_metadata(archive,expected,invalid)
        provenance['checkout_tree']='d'*40
        with self.assertRaisesRegex(ValueError,'provenance'):m.admit_metadata(archive,expected,receipt)


if __name__=='__main__':unittest.main()
