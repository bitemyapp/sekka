"""Independently parse HTML hrefs emitted by the formatter-uri-audit Rust driver.

Build the Rust driver as an example in an isolated core mirror, then pass its
absolute executable path. The target core workspace remains unchanged.
"""
import json,random,subprocess,unicodedata,sys
from html.parser import HTMLParser
rng=random.Random(0x84743893)
parts=[b'%',b'%FF',b'&',b'"',b"'",b'<',b'>',b'\xff',b'\xc3\xa9',b'\xe9\x9b\xaa',b'\xf0\x9f\x94\xa5',b'\xf0\x9f',b'\xe0\x80',b'\xc0\xaf',b'\xed\xa0\x80',b'\xc2\x80',b'\xc2\x9f']
cases=[b'https://example.test/'+p for p in parts]
cases.extend(b'https://example.test/'+b''.join(rng.choices(parts,k=8)) for _ in range(512))
class Links(HTMLParser):
 def __init__(self):super().__init__();self.links=[]
 def handle_starttag(self,tag,attrs):
  if tag=='a':self.links.append(dict(attrs)['href'])
def project(c):
 if 0xdc80<=ord(c)<=0xdcff:return f'%{ord(c)-0xdc00:02X}'
 if unicodedata.category(c)=='Cc':return ''.join(f'%{b:02X}' for b in c.encode('utf-8'))
 return c
p=subprocess.run([sys.argv[1]],input=''.join(json.dumps(list(uri))+'\n' for uri in cases),text=True,capture_output=True,check=True)
for index,(uri,line) in enumerate(zip(cases,p.stdout.splitlines(),strict=True)):
 expected=''.join(map(project,uri.decode('utf-8','surrogateescape')))
 parser=Links();parser.feed(json.loads(line));parser.close()
 assert parser.links==[expected],(index,uri,parser.links,expected)
print(f'{len(cases)} independent HTML-parser href projections passed')
