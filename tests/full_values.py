# SPDX-License-Identifier: MPL-2.0
"""Complete value capture and staged document acceptance over the public wire."""
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from wire import Host

FEATURES = ('view-row-actions', 'view-action-presentation', 'view-metadata', 'view-document', 'job-feedback')

class FullValueTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
  self.path=Path(self.tmp.name)/'values.sqlite'
  with closing(sqlite3.connect(self.path)) as db:
   db.executescript('CREATE TABLE items(id INTEGER PRIMARY KEY, payload TEXT); INSERT INTO items VALUES(1,\'small\');')
  self.h=Host(self.tmp.name,features=FEATURES);self.addCleanup(self.h.close)
 def good(self,result):self.assertNotIn('error',result,result)
 def invoke(self,command,view=None,rows=None,buffer=None):
  result=self.h.invoke(command,view,rows,buffer);self.good(result);return result
 def payload(self,text):
  with closing(sqlite3.connect(self.path)) as db:db.execute('UPDATE items SET payload=? WHERE id=1',(text,));db.commit()
 def record(self):
  h=self.h;self.invoke('connect');self.good(h.submit({'choice':'SQLite'}));self.good(h.submit({'name':'ledger','path':str(self.path)}));h.wait_jobs()
  self.catalog=h.active;self.invoke('activate',h.active,['0']);h.wait_jobs();self.rows=h.active
  self.invoke('activate',self.rows,['0']);return h.active
 def full(self,record):
  self.invoke('show-full',record,['1']);value=self.h.active;self.h.wait_jobs();return value
 def test_large_original_capture_complete_json_raw_and_parent_navigation(self):
  payload='{"members":['+','.join('{"id":1e+02,"id":-0.00,"name":"'+('é'*180)+'"}' for _ in range(12000))+'],"sentinel":"COMPLETE_END猫"}'
  self.assertGreater(len(payload.encode()),4*1024*1024);self.payload(payload)
  record=self.record();self.payload('external replacement')
  value=self.full(record);h=self.h;model=h.views[value]
  self.assertEqual(model['purpose'],'document');self.assertEqual(model['rows'],[])
  self.assertGreater(len(model['document'].splitlines()),10000)
  self.assertIn('COMPLETE_END猫',model['document']);self.assertIn('1e+02',model['document']);self.assertIn('-0.00',model['document'])
  self.assertNotIn('show-full',model['actions']);self.assertFalse(h.stages)
  self.invoke('raw',value);h.wait_jobs();self.assertEqual(h.views[value]['document'],payload)
  self.invoke('back',value);self.assertEqual(h.active,record)
  self.invoke('back',record);self.assertEqual(h.active,self.rows)
  self.assertFalse(h.leases)
 def test_preview_entry_stage_failure_and_retry_preserve_captured_source(self):
  payload='{"text":"'+('x'*150000)+'","end":"original"}';self.payload(payload)
  record=self.record();self.invoke('activate',record,['1']);preview=self.h.active;h=self.h
  h.fail_once='view.stage.write';self.invoke('show-full',preview);failed=h.active;h.wait_jobs()
  self.assertNotIn('document',h.views[failed]);self.assertIn('limit_exceeded',h.views[failed]['status']['text']);self.assertFalse(h.stages)
  self.invoke('raw',failed);self.assertEqual(h.views[failed]['title'].count('payload'),1);self.assertIn('items',h.views[failed]['title'])
  self.payload('changed');self.invoke('show-full',failed);value=h.active;h.wait_jobs()
  self.invoke('raw',value);h.wait_jobs();self.assertEqual(h.views[value]['document'],payload)
  self.invoke('back',value);self.assertEqual(h.active,record)
 def test_cancel_upload_keeps_preview_and_releases_job_for_retry(self):
  self.payload('x'*500000);record=self.record();h=self.h
  def cancel(method):
   if method=='view.stage.write':
    h.before_reply=None;job=next(j for j,state in h.jobs.items() if state=='running')
    h.send({'type':'event','event':'job.cancel_requested','sequence':'100','data':{'job':job}})
  h.before_reply=cancel;self.invoke('show-full',record,['1']);value=h.active;h.wait_jobs()
  self.assertIn('cancelled',h.jobs.values());self.assertNotIn('document',h.views[value]);self.assertFalse(h.stages)
  self.invoke('show-full',value);h.wait_jobs();self.assertEqual(h.views[h.active]['document'],'x'*500000)
 def test_over_limit_value_has_reason_and_no_full_action(self):
  self.payload('x'*(8*1024*1024+1));record=self.record();h=self.h
  self.invoke('activate',record,['1']);value=h.active
  self.assertNotIn('show-full',h.views[value]['actions'])
  self.assertTrue(any(row['label']=='Full value unavailable' for row in h.views[value]['metadata']))
  before=len(h.jobs);self.assertIn('error',h.invoke('show-full',value));self.assertEqual(len(h.jobs),before)
 def test_display_job_does_not_settle_or_replay_writable_returning(self):
  self.record();h=self.h
  self.invoke('mode',self.catalog);self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs()
  self.invoke('query',h.active);buffer=next(reversed(h.buffers))
  h.buffers[buffer]="INSERT INTO items(payload) VALUES(printf('%0100000d',7)) RETURNING id,payload"
  self.invoke('run',buffer=buffer);self.invoke('activate',h.active,['0']);self.good(h.submit({'confirmed':True}));h.wait_jobs()
  result=h.active;self.invoke('activate',result,['0']);record=h.active;leases=dict(h.leases)
  value=self.full(record);self.assertEqual(h.leases,leases);self.assertTrue(leases)
  self.assertEqual(h.views[value]['document'],'0'*99999+'7')
  self.invoke('commit',buffer=buffer);h.wait_jobs()
  with closing(sqlite3.connect(self.path)) as db:self.assertEqual(db.execute('SELECT COUNT(*) FROM items').fetchone()[0],2)
  self.assertEqual(h.views[value]['document'],'0'*99999+'7')

 def test_commit_cancellation_preserves_preview_and_finishes_job(self):
  self.payload('x'*100000);record=self.record();h=self.h;h.cancel_commit=True
  value=self.full(record);self.assertNotIn('document',h.views[value]);self.assertIn('cancelled',h.jobs.values())
  self.assertFalse(h.stages);self.assertIsNone(h.pending_commit)
 def test_toggle_failure_reports_reason_and_preserves_full_document(self):
  self.payload('{"text":"'+'x'*100000+'"}');value=self.full(self.record());h=self.h;before=h.views[value]
  h.fail_once='view.stage.open';self.invoke('raw',value);h.wait_jobs()
  self.assertEqual(h.views[value],before);self.assertIn('limit_exceeded',next(reversed(h.job_messages.values())))
  self.invoke('raw',value);h.wait_jobs();self.assertEqual(h.views[value]['document'],'{"text":"'+'x'*100000+'"}')
 def test_setup_refusal_never_leaves_a_loading_view(self):
  self.payload('x'*100000);record=self.record();h=self.h
  for method in ('job.create','view.create','pane.show'):
   with self.subTest(method=method):
    before=set(h.views);h.fail_once=method;self.assertIn('error',h.invoke('show-full',record,['1']));h.wait_jobs()
    self.assertEqual(set(h.views),before);self.assertFalse(any(state=='running' for state in h.jobs.values()))
 def test_raw_control_whitespace_toggle_keeps_one_stable_title(self):
  payload='{"x":1,\r"y":2}';self.payload(payload);value=self.full(self.record());h=self.h
  original=h.views[value]['title'];self.assertIn('items',original)
  for _ in range(2):
   self.invoke('raw',value);h.wait_jobs();self.assertTrue(h.views[value]['title'].endswith(' · Escaped text'))
   self.invoke('raw',value);h.wait_jobs();self.assertEqual(h.views[value]['title'],original)

if __name__=='__main__':unittest.main()
