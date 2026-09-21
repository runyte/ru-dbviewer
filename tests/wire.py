# SPDX-License-Identifier: MPL-2.0
"""Public-protocol acceptance with actual plugin process and SQLite database.
Run: python3 tests/wire.py [path/to/ru-dbviewer]. Requires jsonschema for tests only.
"""
import json
import copy
from contextlib import closing
import os
from pathlib import Path
import selectors
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent
BINARY = str(Path(sys.argv.pop(1) if len(sys.argv)>1 and not sys.argv[1].startswith('-') else ROOT.parent/'target/debug/ru-dbviewer').resolve())
SCHEMA = json.loads((ROOT/'fixtures/runyte-1.schema.json').read_text())
VALIDATOR = Draft202012Validator({**SCHEMA, 'anyOf':[{'$ref':'#/$defs/pluginMessage'}]})
FIXTURES = json.loads((ROOT/'fixtures/stable-fixtures.json').read_text())
HELLO = next(x['message'] for x in FIXTURES if x['message']['type']=='hello')
REGISTERED = next(x['message'] for x in FIXTURES if x['message']['type']=='registered')

class Host:
 def __init__(self, directory, version='0.3.0', features=(), excluded_env=()):
  self.features=list(features);schema=copy.deepcopy(SCHEMA)
  if 'input-path-completion' in features:schema['$defs']['inputField']['properties']['completion']=json.loads((ROOT/'fixtures/input-path-completion.json').read_text())
  if 'view-row-actions' in features:schema['$defs']['row']['properties']['actions']=json.loads((ROOT/'fixtures/view-row-actions.json').read_text())['row_actions']
  extension=json.loads((ROOT/'fixtures/view-presentation.json').read_text())
  if 'view-action-presentation' in features:
   schema['$defs']['command']['properties']['presentation']=extension['presentation']
   for name in ('model','viewHeader'):
    schema['$defs'][name]['properties']['action_presentation']={'type':'object','maxProperties':64,'additionalProperties':extension['presentation']}
  if 'view-metadata' in features:
   for name in ('model','viewHeader'):schema['$defs'][name]['properties']['metadata']=extension['metadata']
  if 'view-document' in features:
   for name in ('model','viewHeader'):schema['$defs'][name]['properties']['document']={'type':'string','maxLength':8388608}
   schema['$defs']['view.stage.open']['properties']['params']['properties']['bytes']['maximum']=16777216
   schema['$defs']['view.stage.write']['properties']['params']['properties']['offset']['maximum']=16777216
  if 'job-feedback' in features:schema['$defs']['job.finish']['properties']['params']['properties']['message']={'type':'string','minLength':1,'maxLength':1024}
  self.model_validator=Draft202012Validator({'$defs':schema['$defs'],'$ref':'#/$defs/model'})
  self.validator=Draft202012Validator({**schema,'anyOf':[{'$ref':'#/$defs/pluginMessage'}]})
  environment={key:value for key,value in os.environ.items() if key not in excluded_env}
  self.child=subprocess.Popen([BINARY],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,cwd=directory,bufsize=0,env={**environment,'TMPDIR':directory,'XDG_CONFIG_HOME':str(Path(directory)/'config')})
  self.selector=selectors.DefaultSelector();self.selector.register(self.child.stdout,selectors.EVENT_READ)
  self.buffer=bytearray();self.serial=0;self.views={};self.revisions={};self.jobs={};self.inputs={};self.buffers={};self.selection=None;self.leases={};self.state={'revision':'s:missing','document':None};self.active=None;self.reply_log={};self.requests=[];self.stages={};self.stage_serial=0;self.cancel_commit=False;self.pending_commit=None;self.job_messages={};self.fail_once=None;self.fail_code="limit_exceeded";self.before_reply=None
  self.send({**HELLO,'host_version':version,'features':self.features});self.registration=self.read();self.validator.validate(self.registration)
  self.commands={x['name']:x for x in self.registration['commands']}
  self.send({**REGISTERED,'runyte':'>=0.3.0, <0.4.0','capabilities':self.registration['required_capabilities'],'features':[f for f in self.features if f in self.registration['optional_features']]})
 def send(self,msg):self.child.stdin.write((json.dumps(msg)+'\n').encode());self.child.stdin.flush()
 def read(self):
  deadline=time.monotonic()+5
  while b'\n' not in self.buffer:
   assert self.selector.select(max(0,deadline-time.monotonic())), 'plugin response timed out'
   block=os.read(self.child.stdout.fileno(),65536);assert block,'plugin exited early'
   self.buffer.extend(block)
  line,_,self.buffer=self.buffer.partition(b'\n');assert len(line)<1048576
  msg=json.loads(line);self.validator.validate(msg);return msg
 def invoke(self,command,view=None,rows=None,buffer=None,revision=None):
  if view is None and command in ('connect','query','commit','rollback','disconnect','transactions'):command='global-'+command
  self.serial+=1;id=f'h:{self.serial}'
  params={'command':command,'context':self.commands[command]['context'],'arguments':{},'pane':'p:1','selection_revision':'q:1','buffer':buffer,'buffer_revision':'r:1' if buffer else None}
  if view:params.update(view=view,model_revision=revision or self.revisions[view],rows=rows or [])
  self.send({'type':'request','id':id,'method':'command.invoke','params':params});return self.until_reply(id)
 def submit(self,values,accepted=True):
  assert self.inputs;surface=list(self.inputs)[-1];self.inputs.pop(surface);self.serial+=1;id=f'h:{self.serial}'
  self.send({'type':'request','id':id,'method':'ui.submit','params':{'surface':surface,'accepted':accepted,'values':values}});return self.until_reply(id)
 def until_reply(self,id):
  while id not in self.reply_log:self.pump()
  return self.reply_log.pop(id)
 def wait_jobs(self):
  # Drain an accepted operation and its optional follow-up catalog job.
  deadline=time.monotonic()+10
  while any(s=='running' for s in self.jobs.values()):
   assert time.monotonic()<deadline;self.pump()
 def pump(self):
  msg=self.read()
  if msg['type']=='response':self.reply_log[msg['id']]=msg;return
  assert msg['type']=='request';method=msg['method'];p=msg['params'];self.requests.append(method)
  result={};error=None
  if method==self.fail_once:
   self.fail_once=None;self.send({'type':'response','id':msg['id'],'error':{'code':self.fail_code,'message':'fixture rejection'}});return
  if method=='state.get':result=self.state
  elif method=='state.set':assert p['expected_revision']==self.state['revision'];self.state={'revision':'s:'+'a'*64,'document':p['document']};result={'revision':self.state['revision']}
  elif method=='settings.get':result={'settings':{}}
  elif method=='view.create':
   for action in p['model'].get('actions',[]):assert self.commands[action]['context']=='view',action
   id=f'v:{len(self.views)+1}';self.views[id]=p['model'];self.revisions[id]='m:1';result={'view':id,'revision':'m:1'}
  elif method=='pane.show':self.active=p['view']
  elif method=='view.close':self.views.pop(p['view'],None);self.revisions.pop(p['view'],None)
  elif method=='view.publish':
   assert p['expected_revision']==self.revisions[p['view']]
   for action in p['model'].get('actions',[]):assert self.commands[action]['context']=='view',action
   self.views[p['view']]=p['model'];self.revisions[p['view']]=f'm:{int(self.revisions[p["view"]].split(":")[1])+1}';result={'view':p['view'],'revision':self.revisions[p['view']]}
  elif method=='view.stage.open':
   assert p['kind']=='model';assert p['expected_revision']==self.revisions[p['view']]
   self.stage_serial+=1;stage=f'st:{self.stage_serial}';self.stages[stage]={'view':p['view'],'revision':p['expected_revision'],'bytes':p['bytes'],'data':bytearray()};result={'stage':stage,'bytes':p['bytes']}
  elif method=='view.stage.write':
   stage=self.stages[p['stage']];data=p['text'].encode();assert p['offset']==len(stage['data']);assert len(data)<=131072;assert len(stage['data'])+len(data)<=stage['bytes'];stage['data'].extend(data);result={'offset':len(stage['data'])}
  elif method=='view.stage.commit':
   if self.cancel_commit:
    self.cancel_commit=False;self.pending_commit=msg
    job=next(j for j,state in self.jobs.items() if state=='running')
    self.send({'type':'event','event':'job.cancel_requested','sequence':'110','data':{'job':job}});return
   stage=self.stages.pop(p['stage']);view=stage['view'];assert len(stage['data'])==stage['bytes'];model=json.loads(stage['data'].decode());self.model_validator.validate(model)
   if self.revisions.get(view)!=stage['revision']:
    self.send({'type':'response','id':msg['id'],'error':{'code':'conflict','message':'fixture revision changed'}});return
   for action in model.get('actions',[]):assert self.commands[action]['context']=='view'
   self.views[view]=model;self.revisions[view]=f'm:{int(self.revisions[view].split(":")[1])+1}';result={'view':view,'revision':self.revisions[view]}
  elif method=='view.stage.close':
   self.stages.pop(p['stage'],None)
   if self.pending_commit and self.pending_commit['params']['stage']==p['stage']:
    self.send({'type':'response','id':self.pending_commit['id'],'error':{'code':'cancelled','message':'staged preparation cancelled'}});self.pending_commit=None

  elif method=='job.create':id=f'j:{len(self.jobs)+1}';self.jobs[id]='running';result={'job':id}
  elif method=='job.update':pass
  elif method=='job.finish':self.jobs[p['job']]=p['state'];self.job_messages[p['job']]=p.get('message')
  elif method=='job.cancel':self.send({'type':'event','event':'job.cancel_requested','sequence':'1','data':{'job':p['job']}})
  elif method.startswith('ui.'):
   id=f'i:{self.serial}:{len(self.requests)}';self.inputs[id]=p;result={'surface':id}
  elif method=='buffer.create':id=f'b:{len(self.buffers)+1}';self.buffers[id]=p['text'];result={'buffer':id}
  elif method=='buffer.snapshot.open':self.snapshot=self.buffers[p['buffer']];result={'snapshot':'s:1','chars':len(self.snapshot),'revision':'r:1'}
  elif method=='buffer.snapshot.read':result={'text':self.snapshot[p['from']:p['to']]}
  elif method=='buffer.snapshot.close':pass
  elif method=='selection.get':result=self.selection
  elif method=='activity.acquire':id=f'l:{len(self.leases)+1}';self.leases[id]=p;result={'lease':id}
  elif method=='activity.release':self.leases.pop(p['lease'],None)
  else:raise AssertionError(method)
  if self.before_reply:self.before_reply(method)
  self.send({'type':'response','id':msg['id'],'result':result})
 def close(self):
  self.child.stdin.close()
  try:self.child.wait(timeout=4)
  except subprocess.TimeoutExpired:self.child.kill();self.child.wait();raise
  self.child.stdout.close();self.child.stderr.close();self.selector.close()

class WireTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
  self.path=Path(self.tmp.name)/'db.sqlite';c=sqlite3.connect(self.path);c.executescript('CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items VALUES(1,"first"),(2,"second");');c.close()
  self.h=Host(self.tmp.name);self.addCleanup(self.h.close)
 def good(self,r):self.assertNotIn('error',r,r)
 def connect(self):
  h=self.h;self.good(h.invoke('connect'));self.good(h.submit({'choice':'SQLite'}));self.good(h.submit({'name':'test','path':str(self.path)}));h.wait_jobs();self.assertTrue(any('items' in row['text'] for row in h.views[h.active]['rows']))
 def test_browse_query_selection_and_stale_actions(self):
  self.connect();h=self.h;view=h.active
  old=h.revisions[view];self.good(h.invoke('activate',view,['0']));h.wait_jobs();view=h.active;self.assertEqual(h.views[view]['columns'][1]['label'],'name')
  self.assertIn('error',h.invoke('activate',view,['0'],revision=old))
  self.good(h.invoke('activate',view,['0']));view=h.active;self.assertIn('first',h.views[view]['rows'][1]['text'])
  self.good(h.invoke('query',view));buffer=list(h.buffers)[-1];h.buffers[buffer]="SELECT 'é;value' AS value, NULL AS missing;"
  self.good(h.invoke('run',buffer=buffer));h.wait_jobs();self.assertEqual(h.views[h.active]['rows'][0]['cells'][0]['text'],'é;value')
  h.buffers[buffer]='SELECT 1; SELECT 2';self.assertIn('error',h.invoke('run',buffer=buffer))
  h.selection={'revision':'q:1','buffer':buffer,'spans':[{'from':10,'to':18}]}
  self.good(h.invoke('run-selection',buffer=buffer));h.wait_jobs();self.assertEqual(h.views[h.active]['rows'][0]['cells'][0]['text'],'2')
 def test_write_review_commit_and_rollback(self):
  self.connect();h=self.h;self.good(h.invoke('mode',h.active));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs()
  self.good(h.invoke('query',h.active));b=list(h.buffers)[-1]
  for command,id in [('rollback',3),('commit',4)]:
   h.buffers[b]=f"INSERT INTO items VALUES({id}, 'new')";self.good(h.invoke('run',buffer=b));review=h.active
   self.assertIn('captured SQL',h.views[review]['title']);self.good(h.invoke('activate',review,['0']));self.good(h.submit({'confirmed':True}));h.wait_jobs()
   self.assertTrue(h.leases);self.assertIn('PENDING COMMIT',h.views[h.active]['status']['text']);self.good(h.invoke(command,h.active));h.wait_jobs();self.assertFalse(h.leases)
  c=sqlite3.connect(self.path);self.assertEqual(c.execute('select id from items order by id').fetchall(),[(1,),(2,),(4,)]);c.close();self.assertEqual(h.state['document']['data']['uncertain'],[])
 def test_sql_never_runs_when_review_cancelled(self):
  self.connect();h=self.h;self.good(h.invoke('mode',h.active));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs();self.good(h.invoke('query',h.active));b=list(h.buffers)[-1];h.buffers[b]='DELETE FROM items'
  self.good(h.invoke('run',buffer=b));self.good(h.invoke('activate',h.active,['0']));self.good(h.submit({},False));c=sqlite3.connect(self.path);self.assertEqual(c.execute('select count(*) from items').fetchone()[0],2);c.close()

 def test_failed_write_clears_recovery_marker_after_rollback(self):
  self.connect();h=self.h;self.good(h.invoke('mode',h.active));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs();self.good(h.invoke('query',h.active));b=list(h.buffers)[-1]
  h.buffers[b]='INSERT INTO missing_table VALUES(1)';self.good(h.invoke('run',buffer=b));self.good(h.invoke('activate',h.active,['0']));self.good(h.submit({'confirmed':True}));h.wait_jobs()
  self.assertFalse(h.leases);self.assertEqual(h.state['document']['data']['uncertain'],[]);self.assertIn('failed',h.jobs.values())
 def test_activity_cancellation_rolls_back_pending_write(self):
  self.connect();h=self.h;self.good(h.invoke('mode',h.active));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs();self.good(h.invoke('query',h.active));b=list(h.buffers)[-1]
  h.buffers[b]='DELETE FROM items';self.good(h.invoke('run',buffer=b));self.good(h.invoke('activate',h.active,['0']));self.good(h.submit({'confirmed':True}));h.wait_jobs()
  lease=next(iter(h.leases));h.send({'type':'event','event':'activity.cancel_requested','sequence':'2','data':{'lease':lease,'reason':'cancelled'}})
  while h.leases or h.state['document']['data']['uncertain']:h.pump()
  c=sqlite3.connect(self.path);self.assertEqual(c.execute('select count(*) from items').fetchone()[0],2);c.close()
 def test_read_result_limit_and_paging_do_not_execute_again(self):
  self.connect();h=self.h;self.good(h.invoke('query',h.active));b=list(h.buffers)[-1]
  h.buffers[b]='WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1100) SELECT x FROM n'
  self.good(h.invoke('run',buffer=b));h.wait_jobs();v=h.active;self.assertIn('Result incomplete',h.views[v]['status']['text']);jobs=len(h.jobs)
  self.good(h.invoke('next',v));self.assertEqual(h.views[v]['rows'][0]['cells'][0]['text'],'101');self.assertEqual(len(h.jobs),jobs)
 def test_large_frames_drain_without_waiting_for_another_frame(self):
  # Force short writes where supported; no writer may buffer the frame's final newline.
  import fcntl
  if hasattr(fcntl,'F_SETPIPE_SZ'):fcntl.fcntl(self.h.child.stdout.fileno(),fcntl.F_SETPIPE_SZ,4096)
  self.connect();h=self.h;self.good(h.invoke('query',h.active));b=list(h.buffers)[-1]
  h.buffers[b]="WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<101) SELECT x, printf('%0500d',x) FROM n"
  self.good(h.invoke('run',buffer=b));h.wait_jobs();v=h.active;self.assertEqual(len(h.views[v]['rows']),100)
  self.good(h.invoke('next',v));self.assertEqual(h.views[v]['rows'][0]['cells'][0]['text'],'101')
 def test_multiple_selections_and_changed_selection_are_refused(self):
  self.connect();h=self.h;self.good(h.invoke('query',h.active));b=list(h.buffers)[-1]
  for revision,spans in [('q:2',[{'from':0,'to':8}]),('q:1',[{'from':0,'to':1},{'from':2,'to':4}])]:
   h.selection={'revision':revision,'buffer':b,'spans':spans};self.assertIn('error',h.invoke('run-selection',buffer=b))

 def test_reconnect_requires_explicit_buffer_reassociation(self):
  self.connect();h=self.h;catalog=h.active;self.good(h.invoke('query',catalog));b=list(h.buffers)[-1]
  self.good(h.invoke('mode',catalog));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs()
  self.assertIn('error',h.invoke('run',buffer=b));self.good(h.invoke('use',buffer=b));self.good(h.submit({'choice':'test'}));self.good(h.invoke('run',buffer=b));self.assertIn('captured SQL',h.views[h.active]['title'])
 def test_refused_activity_or_job_never_executes_sql(self):
  self.connect();h=self.h;self.good(h.invoke('mode',h.active));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs();self.good(h.invoke('query',h.active));b=list(h.buffers)[-1];h.buffers[b]='DELETE FROM items'
  for method in ('state.set','activity.acquire','job.create'):
   self.good(h.invoke('run',buffer=b));self.good(h.invoke('activate',h.active,['0']));h.fail_once=method
   self.assertIn('error',h.submit({'confirmed':True}));self.assertFalse(h.leases);self.assertEqual(h.state['document']['data']['uncertain'],[])
   c=sqlite3.connect(self.path);self.assertEqual(c.execute('SELECT COUNT(*) FROM items').fetchone()[0],2);c.close()

 def test_callback_window_during_host_request(self):
  h=self.h
  def burst(method):
   if method!='state.set':return
   h.before_reply=None
   for i in range(16):h.send({'type':'request','id':f'burst:{i}','method':'unknown.fixture','params':{}})
   for i in range(12):h.send({'type':'event','event':'view.closed','sequence':str(i+1),'data':{'view':f'v:closed{i}'}})
   for i in range(2):h.send({'type':'event','event':'activity.cancel_requested','sequence':str(i+13),'data':{'lease':f'l:expired{i}','reason':'cancelled'}})
   time.sleep(0.1)
  h.before_reply=burst
  self.connect();self.good(h.invoke('refresh',h.active));h.wait_jobs()
  self.assertTrue(all(f'burst:{i}' in h.reply_log for i in range(16)))
 def test_closed_view_publication_does_not_stop_plugin(self):
  self.connect();h=self.h
  for code in ('not_found','closed','cancelled'):
   with self.subTest(code=code):
    self.good(h.invoke('query'));b=list(h.buffers)[-1];h.buffers[b]='SELECT 42'
    h.fail_once='view.publish';h.fail_code=code
    self.good(h.invoke('run',buffer=b));h.wait_jobs()
    self.assertTrue(all(state=='succeeded' for state in h.jobs.values()))
 def test_closed_view_preserves_pending_write(self):
  self.connect();h=self.h;self.good(h.invoke('mode',h.active));self.good(h.submit({'choice':'READ AND WRITE'}));self.good(h.submit({'confirmed':True}));h.wait_jobs()
  self.good(h.invoke('query'));b=list(h.buffers)[-1]
  for i,code in enumerate(('not_found','closed','cancelled')):
   with self.subTest(code=code):
    h.buffers[b]=f"INSERT INTO items VALUES({i+3}, 'retained')"
    self.good(h.invoke('run',buffer=b));self.good(h.invoke('activate',h.active,['0']))
    h.fail_once='view.publish';h.fail_code=code
    self.good(h.submit({'confirmed':True}));h.wait_jobs()
    self.assertTrue(h.leases);self.assertTrue(all(state=='succeeded' for state in h.jobs.values()))
    self.good(h.invoke('commit',buffer=b));h.wait_jobs();self.assertFalse(h.leases)
  with closing(sqlite3.connect(self.path)) as c:self.assertEqual(c.execute('SELECT count(*) FROM items').fetchone()[0],5)
 def test_empty_column_labels_in_queries_and_tables(self):
  with closing(sqlite3.connect(self.path)) as c:
   c.execute('CREATE TABLE empty_name("" TEXT)');c.execute("INSERT INTO empty_name VALUES('value')");c.commit()
  self.connect();h=self.h;v=h.active
  self.good(h.invoke('activate',v,['0']));h.wait_jobs();v=h.active;self.assertEqual(h.views[v]['columns'][0]['label'],'(unnamed)')
  self.good(h.invoke('query'));b=list(h.buffers)[-1];h.buffers[b]='SELECT 1 AS "", 2 AS ""'
  self.good(h.invoke('run',buffer=b));h.wait_jobs();cols=h.views[h.active]['columns']
  self.assertEqual([c['label'] for c in cols],['(unnamed)','(unnamed)']);self.assertNotEqual(cols[0]['id'],cols[1]['id'])
 def test_wide_record_keeps_every_column_selectable(self):
  # Backslashes and quotes increase encoded JSON size beyond plain text size.
  value=('\\"é'*700)
  with closing(sqlite3.connect(self.path)) as c:
   c.execute('CREATE TABLE wide('+','.join(f'c{i} TEXT' for i in range(600))+')')
   c.execute('INSERT INTO wide VALUES('+','.join('?' for _ in range(600))+')',[value]*600);c.commit()
  self.connect();h=self.h;self.good(h.invoke('query'));b=list(h.buffers)[-1];h.buffers[b]='SELECT * FROM wide'
  self.good(h.invoke('run',buffer=b));h.wait_jobs();v=h.active;jobs=len(h.jobs)
  self.good(h.invoke('activate',v,['0']));v=h.active;rows=h.views[v]['rows']
  self.assertEqual(len(rows),600);self.assertEqual(rows[-1]['id'],'599')
  self.assertLess(len(json.dumps(h.views[v],ensure_ascii=False).encode()),700000)
  self.good(h.invoke('activate',v,['599']));self.assertEqual(''.join(r['text'] for r in h.views[h.active]['rows']),value)
  self.assertEqual(len(h.jobs),jobs)
 def test_control_character_table_title(self):
  c=sqlite3.connect(self.path);c.execute('CREATE TABLE "a\nb"(value TEXT)');c.close()
  self.connect();h=self.h;v=h.active
  self.good(h.invoke('activate',v,['0']));h.wait_jobs();v=h.active
  self.assertIn(r'a\nb',h.views[v]['title']);self.assertFalse(any(ord(c)<32 for c in h.views[v]['title']))
 def test_refresh_preserves_page_and_schema_shows_metadata(self):
  c=sqlite3.connect(self.path);c.executemany('INSERT INTO items VALUES(?,?)',[(i,str(i)) for i in range(3,230)]);c.commit();c.close()
  self.connect();h=self.h;v=h.active;self.good(h.invoke('activate',v,['0']));h.wait_jobs();v=h.active
  self.good(h.invoke('next',v));h.wait_jobs();before=h.views[v]['rows']
  self.good(h.invoke('refresh',v));h.wait_jobs();self.assertEqual(h.views[v]['rows'],before);self.assertIn('Rows: 101–200',h.views[v]['status']['text'])
  self.assertIn('schema',h.views[v]['actions']);self.good(h.invoke('schema',v));h.wait_jobs();v=h.active
  self.assertEqual(h.views[v]['columns'][0]['label'],'kind');self.assertEqual(len(h.views[v]['rows']),2)

if __name__=='__main__':unittest.main()
