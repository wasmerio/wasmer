import subprocess

result = subprocess.run(
    ["/bin/python3.13", "/code/child.py"], capture_output=True, text=True
)

print(f"{result.returncode}", end="")
