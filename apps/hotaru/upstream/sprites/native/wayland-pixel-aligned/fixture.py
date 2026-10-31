import fcntl,json,os,pathlib,struct,sys,termios,time
path=pathlib.Path(sys.argv[1])
while True:
    rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))
    if rows>=31 and cols>=52:break
    time.sleep(.02)
cp=[c for c in range(0x2500,0x25a0) if not 0x256d<=c<=0x2573]
payload='\x1b[?25l\x1b]11;#000000\x07\x1b[2J\x1b[H\x1b[38;2;255;0;255m█'
payload+='\x1b[38;2;255;255;255m\x1b]8;id=sprite-fixture;https://example.test/sprites\x1b\\'
for i,c in enumerate(cp):
    payload+=f'\x1b[{3+3*(i//17)};{3+3*(i%17)}H'+chr(c)
payload+='\x1b]8;;\x1b\\'
payload+='\x1b[31;1H\x1b[1;3;4;9;38;2;0;255;0mAB┼CD╭─\u0301\x1b[0m'
os.write(1,payload.encode())
while True:
    size=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))
    tmp=path.with_suffix('.tmp');tmp.write_text(json.dumps(size));tmp.replace(path)
    time.sleep(.02)
