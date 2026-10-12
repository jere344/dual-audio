import os

from dual_audio import make_icon_image

os.makedirs("assets", exist_ok=True)
image = make_icon_image(256)
image.save("assets/icon.ico")
image.save("assets/icon.png")
print("Wrote assets/icon.ico and assets/icon.png")
