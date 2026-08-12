import subprocess, os, zipfile
jvm = r"C:\Program Files\Amazon Corretto\jdk1.8.0_492\bin\javap.exe"
jar = r"C:\Users\Administrator\.m2\repository\com\chua\utils-support-runtime-starter\4.0.0.42\utils-support-runtime-starter-4.0.0.42.jar"
out = r"C:\Users\Administrator\AppData\Local\Temp\RB"
with zipfile.ZipFile(jar) as z:
    with z.open("com/chua/runtime/starter/RuntimeBoot.class") as src, open(out + ".class", "wb") as dst:
        dst.write(src.read())
r = subprocess.run([jvm, "-c", "-p", out + ".class"], capture_output=True, text=True)
print(r.stdout[:5000])
