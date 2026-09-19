# SPDX-License-Identifier: MPL-2.0
"""Interactive browsing over the public wire and disposable SQLite databases."""
from contextlib import closing
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from wire import Host

class InteractiveTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name);self.path=self.root/'ledger.sqlite'
  with closing(sqlite3.connect(self.path)) as c:
   c.executescript("CREATE TABLE items(id INTEGER PRIMARY KEY,name TEXT,payload TEXT); INSERT INTO items VALUES(1,'alpha','{\"n\":123456789012345678901234567890,\"a\":[1,2]}'),(2,'beta',NULL),(3,'','{}'),(4,'100% literal','[]');")
  self.h=Host(self.tmp.name,features=getattr(self,"features",()));self.addCleanup(self.h.close)
 def good(self,r):self.assertNotIn('error',r,r)
 def invoke(self,cmd,view=None,rows=None,buffer=None):self.good(self.h.invoke(cmd,view,rows,buffer))
 def submit(self,**values):self.good(self.h.submit(values))
 def choice(self,text):
  choices=list(self.h.inputs.values())[-1]['choices'];choice=next(c for c in choices if text in c);self.submit(choice=choice)
 def connect(self):
  self.invoke('connect');self.submit(choice='SQLite');self.submit(name='ledger',path=str(self.path));self.h.wait_jobs();return self.h.active
 def rows(self):
  catalog=self.connect();self.invoke('activate',catalog,['0']);self.h.wait_jobs();return self.h.active
 def filter(self,view,column,op,value=None):
  self.invoke('add-filter',view);self.choice(column);self.submit(choice=op)
  if value is not None:self.submit(value=value)
 def test_navigation_json_raw_and_closed_parent(self):
  rows=self.rows();h=self.h;before=h.views[rows];jobs=len(h.jobs)
  self.invoke('activate',rows,['0']);record=h.active
  self.assertIn('disconnect',h.views[record]['actions']);self.invoke('activate',record,['2']);value=h.active
  self.assertIn('123456789012345678901234567890',json.dumps(h.views[value]));self.invoke('activate',value,['0']);self.assertEqual(len(h.views[value]['rows']),1)
  self.invoke('activate',value,['0']);self.invoke('raw',value);self.assertIn('Original retained',h.views[value]['status']['text'])
  self.invoke('back',value);self.assertEqual(h.active,record);self.invoke('back',record);self.assertEqual(h.active,rows);self.assertEqual(h.views[rows],before);self.assertEqual(len(h.jobs),jobs)
  h.send({'type':'event','event':'view.closed','sequence':'1','data':{'view':record}});self.invoke('back',value);self.assertEqual(h.views[h.active]['title'],'Databases')
 def test_filters_all_any_disabled_sort_page_and_generated_sql(self):
  rows=self.rows();h=self.h;self.invoke('filters',rows);filters=h.active
  self.filter(filters,'name','contains (literal)','a');self.filter(filters,'id','greater than','1')
  self.invoke('apply-filters',filters);h.wait_jobs();self.assertEqual([r['cells'][0]['text'] for r in h.views[rows]['rows']],['2','4'])
  self.invoke('sort',rows);self.choice('id');self.submit(choice='Descending');h.wait_jobs();self.assertEqual(h.views[rows]['rows'][0]['cells'][0]['text'],'4')
  self.invoke('page-size',rows);self.submit(size='1');h.wait_jobs();self.assertIn('page size 1',h.views[rows]['status']['text']);self.invoke('next',rows);h.wait_jobs();self.assertEqual(h.views[rows]['rows'][0]['cells'][0]['text'],'2')
  jobs=len(h.jobs);self.invoke('browse-sql',rows);self.assertEqual(len(h.jobs),jobs);b=list(h.buffers)[-1];sql=h.buffers[b];self.assertNotIn('?',sql);self.assertIn('ORDER BY',sql)
  with closing(sqlite3.connect(self.path)) as c:self.assertEqual(c.execute(sql).fetchall()[0][0],2)
  self.invoke('return',buffer=b);self.assertEqual(h.active,rows)
  self.invoke('match',filters);self.submit(choice='Match ANY (OR)');self.invoke('toggle-filter',filters,['1']);self.invoke('apply-filters',filters);h.wait_jobs();self.assertEqual(len(h.views[rows]['rows']),1);self.assertIn('page size 1',h.views[rows]['status']['text'])
  self.invoke('remove-filter',filters,['1']);self.invoke('clear-filters',filters);self.invoke('apply-filters',filters);h.wait_jobs();self.assertEqual(len(h.views[rows]['rows']),1);self.assertEqual(h.views[rows]['rows'][0]['cells'][0]['text'],'4')
 def test_null_empty_literal_and_edit(self):
  rows=self.rows();h=self.h;self.invoke('filters',rows);f=h.active
  self.filter(f,'payload','is NULL');self.invoke('apply-filters',f);h.wait_jobs();self.assertEqual(h.views[rows]['rows'][0]['cells'][0]['text'],'2')
  self.invoke('edit-filter',f,['0']);self.choice('name');self.submit(choice='equals');self.submit(value='');self.invoke('apply-filters',f);h.wait_jobs();self.assertEqual(h.views[rows]['rows'][0]['cells'][0]['text'],'3')
  self.invoke('clear-filters',f);self.filter(f,'name','contains (literal)','%');self.invoke('apply-filters',f);h.wait_jobs();self.assertEqual([r['cells'][0]['text'] for r in h.views[rows]['rows']],['4'])
 def test_columns_search_query_guidance_and_collision(self):
  rows=self.rows();h=self.h;self.invoke('columns',rows);self.choice('Find column');self.submit(search='payload');self.choice('payload');self.choice('Apply selections');self.assertEqual(len(h.views[rows]['columns']),2)
  self.invoke('columns',rows);self.choice('Find column');self.submit(search='payload');self.choice('payload');self.choice('Apply selections');self.assertEqual(len(h.views[rows]['columns']),3)
  self.invoke('columns',rows);self.choice('id');self.choice('Apply selections');self.invoke('refresh',rows);h.wait_jobs();self.assertEqual([c['label'] for c in h.views[rows]['columns']],['name','payload'])
  h.fail_once='buffer.create';h.fail_code='conflict';self.invoke('query',rows);self.invoke('query',rows);self.assertEqual(len(h.buffers),2)
  for text in h.buffers.values():self.assertIn('::db-return',text);self.assertIn('including unsaved edits',text)
  self.assertEqual(list(self.root.glob('*.sql')),[])
 def test_path_completion_and_validation_keeps_surface(self):
  h=self.h;self.invoke('connect');self.submit(choice='SQLite');surface=next(iter(h.inputs))
  for revision,name,path,expected in [('r:1','','missing','invalid'),('r:2','ledger','led','valid')]:
   h.serial+=1;id=f'h:{h.serial}';h.send({'type':'request','id':id,'method':'ui.validate','params':{'surface':surface,'revision':revision,'fields':['name','path'],'values':{'name':name,'path':path}}});r=h.until_reply(id);self.assertEqual(r['result']['revision'],revision);self.assertEqual(r['result']['fields'][1]['status'],expected);self.assertIn(surface,h.inputs)
  self.submit(name='ledger',path='led');self.choice('ledger.sqlite');h.wait_jobs();self.assertIn('items',str(h.views[h.active]))
 def test_profile_menu_tracks_selected_connection_and_availability(self):
  self.connect();h=self.h;self.invoke('open');profiles=h.active
  self.invoke('profile-actions',profiles,['0']);self.assertIn('disconnect',list(h.inputs.values())[-1]['choices']);self.submit(choice='mode');self.assertEqual(list(h.inputs.values())[-1]['choices'],['READ ONLY','READ AND WRITE']);self.good(h.submit({},False))
  self.invoke('profile-actions',profiles,['0']);self.submit(choice='query');self.assertIn('-- ledger',h.buffers[list(h.buffers)[-1]]);self.invoke('return',buffer=list(h.buffers)[-1]);self.assertEqual(h.active,profiles)
  self.invoke('profile-actions',profiles,['0']);self.submit(choice='transactions');self.invoke('back',h.active);self.assertEqual(h.active,profiles)
  self.invoke('profile-actions',profiles,['0']);self.submit(choice='disconnect');self.invoke('profile-actions',profiles,['0']);self.assertEqual(list(h.inputs.values())[-1]['choices'],['connect']);self.submit(choice='connect');h.wait_jobs();self.assertIn('ledger',h.views[h.active]['title'])

 def test_password_prompt_keeps_profile_list_parent(self):
  # A refused local port still publishes the connecting catalog before failure;
  # its parent identity must survive the password surface.
  h=self.h;h.state['document']={'version':1,'data':{'profiles':[{'backend':'postgres','name':'fixture','host':'127.0.0.1','port':1,'database':'fixture','user':'fixture','plaintext':True}],'uncertain':[]}}
  self.invoke('open');profiles=h.active;self.invoke('profile-actions',profiles,['0']);self.submit(choice='connect');self.assertIn('password',list(h.inputs.values())[-1]['title']);self.submit(password='fixture');h.wait_jobs();catalog=h.active
  self.invoke('back',catalog);self.assertEqual(h.active,profiles)

 def test_deep_json_obeys_public_limits_and_profile_validation_bytes(self):
  catalog=self.connect();h=self.h;self.invoke('query',catalog);b=list(h.buffers)[-1]
  text='['*50+'0'+']'*50;h.buffers[b]="SELECT '"+text+"' AS tree";self.invoke('run',buffer=b);h.wait_jobs();self.invoke('activate',h.active,['0']);self.invoke('activate',h.active,['0']);self.assertEqual(len(h.views[h.active]['rows']),51)
  self.assertTrue(all(len(r['id'])<=64 for r in h.views[h.active]['rows']));self.invoke('activate',h.active,['0']);self.assertEqual(len(h.views[h.active]['rows']),1)
  self.invoke('connect');self.submit(choice='SQLite');surface=next(iter(h.inputs))
  for name in ['é'*40,'bad\nname']:
   h.serial+=1;id=f'h:{h.serial}';h.send({'type':'request','id':id,'method':'ui.validate','params':{'surface':surface,'revision':'r:1','fields':['name','path'],'values':{'name':name,'path':str(self.path)}}});reply=h.until_reply(id);self.assertEqual(reply['result']['fields'][0]['status'],'invalid');self.assertIn(surface,h.inputs)

 def test_transactions_pending_mode_and_disconnect_cancel(self):
  catalog=self.connect();h=self.h;self.assertNotIn('acknowledge',h.views[catalog]['actions']);self.invoke('mode',catalog);self.submit(choice='READ AND WRITE');self.submit(confirmed=True);h.wait_jobs();catalog=h.active
  self.invoke('query',catalog);b=list(h.buffers)[-1];h.buffers[b]="INSERT INTO items VALUES(5,'pending','{}')";self.invoke('run',buffer=b);self.invoke('activate',h.active,['0']);self.submit(confirmed=True);h.wait_jobs()
  result=h.active;self.invoke('mode',catalog);self.assertIn('error',h.submit({'choice':'READ ONLY'}));self.invoke('transactions',result);tx=h.active;self.assertIn('pending',str(h.views[tx]).lower());self.assertIn('1 affected',str(h.views[tx]))
  self.invoke('disconnect',tx,['0']);self.good(h.submit({},False));self.assertTrue(h.leases)
  self.invoke('commit',tx,['0']);h.wait_jobs();self.assertFalse(h.leases);self.assertEqual(h.views[tx]['rows'],[])
  self.assertIn('error',h.invoke('commit',tx,['0']));self.invoke('mode',catalog);self.submit(choice='READ ONLY');h.wait_jobs();self.assertIn('error',h.invoke('run',buffer=b))
 def test_selected_profile_owns_actions_and_disconnect(self):
  self.connect();h=self.h;other=self.root/'other.sqlite';sqlite3.connect(other).close();self.invoke('connect');self.submit(choice='SQLite');self.submit(name='other',path=str(other));h.wait_jobs();self.invoke('open');profiles=h.active
  self.invoke('activate',profiles,['0']);h.wait_jobs();self.assertIn('ledger',h.views[h.active]['title']);self.invoke('back',h.active);self.assertEqual(h.active,profiles)
  self.invoke('query',profiles,['0']);b=list(h.buffers)[-1];self.assertIn('-- ledger',h.buffers[b]);self.invoke('disconnect',profiles,['0']);self.assertIn('error',h.invoke('run',buffer=b));self.assertIn('disconnected',h.views[profiles]['rows'][0]['text']);self.assertIn('ready',h.views[profiles]['rows'][1]['text'])

class RowActionTests(InteractiveTests):
 # Run the interactive workflows against both old and feature-aware hosts.
 features=['view-row-actions']
 def test_direct_profile_actions_follow_connection_and_transaction_state(self):
  h=self.h
  self.connect()
  self.invoke('open');databases=h.active
  actions=h.views[databases]['rows'][0]['actions']
  self.assertIn('view-row-actions',h.registration['optional_features'])
  self.assertIn('disconnect',actions);self.assertIn('mode',actions)
  self.assertNotIn('profile-actions',h.views[databases]['actions'])
  self.assertNotIn('commit',actions);self.assertNotIn('acknowledge',actions)
  self.invoke('query',databases,['0']);buffer=list(h.buffers)[-1]
  self.invoke('return',buffer=buffer);self.assertEqual(h.active,databases)
  self.invoke('disconnect',databases,['0'])
  actions=h.views[databases]['rows'][0]['actions']
  self.assertIn('connect',actions);self.assertNotIn('disconnect',actions)
  self.invoke('connect',databases,['0']);h.wait_jobs()
  self.invoke('back',h.active);self.assertEqual(h.active,databases)
  self.invoke('mode',databases,['0']);self.submit(choice='READ AND WRITE');self.submit(confirmed=True);h.wait_jobs()
  self.invoke('query',databases,['0']);buffer=list(h.buffers)[-1];h.buffers[buffer]='DELETE FROM items'
  self.invoke('run',buffer=buffer);self.invoke('activate',h.active,['0']);self.submit(confirmed=True);h.wait_jobs()
  actions=h.views[databases]['rows'][0]['actions'];self.assertIn('commit',actions);self.assertIn('rollback',actions)
  self.invoke('rollback',databases,['0']);h.wait_jobs()
  self.assertNotIn('commit',h.views[databases]['rows'][0]['actions'])

if __name__=='__main__':unittest.main()
