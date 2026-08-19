import subprocess

result = subprocess.run(["/bin/python", '/code/child.py'], capture_output=True, text=True)

print(f"{result.returncode}", end="")