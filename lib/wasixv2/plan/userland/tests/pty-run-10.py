#!/usr/bin/env python3
"""Give the CLI an isolated foreground controlling PTY for upstream tty tests."""
import errno
import fcntl
import os
import select
import signal
import struct
import subprocess
import sys
import termios
import time

master, slave = os.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
def session():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)
child = subprocess.Popen(sys.argv[1:], stdin=slave, stdout=slave, stderr=slave,
                         preexec_fn=session)
os.close(slave)
start = time.monotonic()
try:
    while True:
        if time.monotonic() - start > 1000:
            os.killpg(child.pid, signal.SIGKILL)
        if select.select([master], [], [], .2)[0]:
            try:
                data = os.read(master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                break
            if not data:
                break
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
        elif child.poll() is not None:
            break
finally:
    os.close(master)
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGKILL)
status = child.wait()
sys.exit(status if status >= 0 else 128 - status)
