import os
import re

with open(r"D:\ch\project\vue-support-parent-starter\pages\gateway\src\views\RemoteControl.vue", "r", encoding="utf-8") as f:
    content = f.read()

imports = re.findall(r"from\s+['\"](\.\./[^'\"]+)['\"]", content)
for imp in imports:
    base = r"D:\ch\project\vue-support-parent-starter\pages\gateway\src"
    resolved = os.path.join(base, *imp.split("/")[1:])
    exists = os.path.exists(resolved)
    print(f"  {'OK     ' if exists else 'MISSING'} {imp}")

print("---")
print("Existing components in pages/gateway/src/components:")
for f in os.listdir(r"D:\ch\project\vue-support-parent-starter\pages\gateway\src\components"):
    print(f"  {f}")
