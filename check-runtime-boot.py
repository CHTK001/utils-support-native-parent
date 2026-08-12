"""
Check RuntimeBoot.install() to see if it actually downloads guacd
"""
import subprocess, os
# Use javap to see bytecode
jvm = "C:\Program Files\Amazon Corretto\jdk1.8.0_492\bin\javap.exe"
jar = "C:\Users\Administrator\.m2\repository\com\chua\utils-support-runtime-starter\4.0.0.42\utils-support-runtime-starter-4.0.0.42.jar"

# Extract using another method
import zipfile
out = r"C:\Users\Administrator\AppData\Local\Temp\RB"
with zipfile.ZipFile(jar) as z:
    with z.open("com/chua/runtime/starter/RuntimeBoot.class") as src, open(out + ".class", "wb") as dst:
        dst.write(src.read())
import subprocess
r = subprocess.run([jvm, "-c", "-p", out + ".class"], capture_output=True, text=True)
print(r.stdout[:5000])
