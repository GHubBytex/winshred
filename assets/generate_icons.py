from PIL import Image
import os

base_dir = os.path.dirname(os.path.abspath(__file__)) # assets/
root_dir = os.path.dirname(base_dir) # repo root
img_path = os.path.join(base_dir, "Gemini_Generated_Image_3wywxm3wywxm3wyw.jpg")

if not os.path.exists(img_path):
    print("Bilddatei nicht gefunden:", img_path)
    exit(1)

img = Image.open(img_path).convert("RGBA")

# Bounding Box des Icons ist ca. [491, 388, 1557, 1683]
# Zentrierter Beschnitt (Zoom) auf das Motiv ohne ueberfluessige Raender:
box_size = 1450
cx, cy = 1024, 1035
crop_rect = (
    int(cx - box_size / 2),
    int(cy - box_size / 2),
    int(cx + box_size / 2),
    int(cy + box_size / 2)
)
cropped = img.crop(crop_rect)

# 1. Assets fuer Windows 11 Sparse Package
assets_dir = os.path.join(root_dir, "packaging", "package_src", "Assets")
os.makedirs(assets_dir, exist_ok=True)

cropped.resize((150, 150), Image.Resampling.LANCZOS).save(os.path.join(assets_dir, "Square150x150Logo.png"), "PNG")
cropped.resize((44, 44), Image.Resampling.LANCZOS).save(os.path.join(assets_dir, "Square44x44Logo.png"), "PNG")
cropped.resize((50, 50), Image.Resampling.LANCZOS).save(os.path.join(assets_dir, "StoreLogo.png"), "PNG")

# 2. Multi-Resolution ICO Datei (16 bis 256 px)
ico_sizes = [(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)]
ico_path = os.path.join(base_dir, "winshred.ico")
cropped.save(ico_path, format="ICO", sizes=ico_sizes)

shell_ico_path = os.path.join(root_dir, "winshred_shell", "winshred.ico")
cropped.save(shell_ico_path, format="ICO", sizes=ico_sizes)

print("Gezoomte Icons erfolgreich generiert!")
