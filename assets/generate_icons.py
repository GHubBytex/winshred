from PIL import Image
import os

base_dir = os.path.dirname(os.path.abspath(__file__)) # assets/
root_dir = os.path.dirname(base_dir) # repo root
img_path = os.path.join(base_dir, "icon.png")

if not os.path.exists(img_path):
    print("Bilddatei nicht gefunden:", img_path)
    exit(1)

img = Image.open(img_path).convert("RGBA")

# Quadratischer Zuschnitt (falls nötig)
w, h = img.size
if w != h:
    size = min(w, h)
    left = (w - size) // 2
    top = (h - size) // 2
    img = img.crop((left, top, left + size, top + size))

# 1. Assets für Windows 11 Sparse Package
assets_dir = os.path.join(root_dir, "packaging", "package_src", "Assets")
os.makedirs(assets_dir, exist_ok=True)

img.resize((150, 150), Image.Resampling.LANCZOS).save(os.path.join(assets_dir, "Square150x150Logo.png"), "PNG")
img.resize((44, 44), Image.Resampling.LANCZOS).save(os.path.join(assets_dir, "Square44x44Logo.png"), "PNG")
img.resize((50, 50), Image.Resampling.LANCZOS).save(os.path.join(assets_dir, "StoreLogo.png"), "PNG")

# 2. Multi-Resolution ICO Datei (16 bis 256 px)
ico_sizes = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)]
ico_path = os.path.join(base_dir, "winshred.ico")
img.save(ico_path, format="ICO", sizes=ico_sizes)

shell_ico_path = os.path.join(root_dir, "winshred_shell", "winshred.ico")
img.save(shell_ico_path, format="ICO", sizes=ico_sizes)

print("Icons erfolgreich aus icon.png generiert!")

