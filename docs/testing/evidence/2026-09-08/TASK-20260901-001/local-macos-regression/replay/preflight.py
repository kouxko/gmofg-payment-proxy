import socket,selectors,time
sel=selectors.DefaultSelector()
sockets=[]
try:
 for port in (28081,28083):
  s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(('127.0.0.1',port)); s.listen(8); sockets.append(s); sel.register(s,selectors.EVENT_READ)
 print('READY validation-only TCP endpoints; no business data',flush=True)
 end=time.monotonic()+180
 while time.monotonic()<end:
  for key,_ in sel.select(.2):
   conn,peer=key.fileobj.accept(); conn.close(); print('preflight connection',peer,flush=True)
finally:
 for s in sockets:s.close()
 sel.close()
