"""Generate platform icon assets from the selected AI artwork (requires Pillow)."""
from pathlib import Path
from PIL import Image
import struct

PROJECT = Path(__file__).resolve().parents[1]
ICONS = PROJECT / 'src-tauri' / 'icons'
PUBLIC = PROJECT / 'public'
source = Image.open(ICONS / 'fushi-master.png').convert('RGBA')

# Fit the visible rounded-square artwork to both canvas edges. The original
# artwork is slightly wider than tall; keeping that ratio leaves extra space
# above and below the icon at desktop/taskbar sizes.
_alpha = source.getchannel('A').point(lambda value: 255 if value > 8 else 0)
_bbox = _alpha.getbbox()
if _bbox:
    source = source.crop(_bbox).resize(source.size, Image.Resampling.LANCZOS)

def resized(size):
    return source.resize((size, size), Image.Resampling.LANCZOS)

for name, size in {'32x32.png':32, '128x128.png':128, '128x128@2x.png':256,
                   'icon.png':512, 'icon_resize.png':1024, 'StoreLogo.png':50}.items():
    resized(size).save(ICONS / name)
for size in [30,44,71,89,107,142,150,284,310]:
    resized(size).save(ICONS / f'Square{size}x{size}Logo.png')

# Tauri's Windows decoder uses the first entry for the window icon.
# Store the highest resolution first, with conventional 32-bit DIB entries.
sizes = [256,128,96,64,48,40,36,32,24,20,16]
entries = []
for size in sizes:
    image = resized(size)
    pixels = image.transpose(Image.Transpose.FLIP_TOP_BOTTOM).tobytes('raw', 'BGRA')
    stride = ((size + 31) // 32) * 4
    mask = bytearray(stride * size)
    alpha = image.getchannel('A')
    for y in range(size):
        for x in range(size):
            if alpha.getpixel((x, size - 1 - y)) == 0:
                mask[y * stride + x // 8] |= 0x80 >> (x % 8)
    header = struct.pack('<IiiHHIIiiII',40,size,size*2,1,32,0,len(pixels),0,0,0,0)
    entries.append(header + pixels + mask)
offset = 6 + 16 * len(sizes)
directory = bytearray(struct.pack('<HHH',0,1,len(sizes)))
for size, entry in zip(sizes, entries):
    directory.extend(struct.pack('<BBBBHHII',size if size < 256 else 0,size if size < 256 else 0,0,0,1,32,len(entry),offset))
    offset += len(entry)
ico = bytes(directory) + b''.join(entries)
(ICONS / 'icon.ico').write_bytes(ico)
(PUBLIC / 'favicon.ico').write_bytes(ico)
# A separate shortcut icon path also avoids reusing Explorer's old EXE icon.
shortcut_icon = PROJECT / 'src-tauri' / 'assets' / 'ukiyo-fullsize.ico'
shortcut_icon.parent.mkdir(parents=True, exist_ok=True)
shortcut_icon.write_bytes(ico)
resized(1024).save(ICONS / 'icon.icns', format='ICNS')
resized(192).save(PUBLIC / 'logo192.png')
resized(512).save(PUBLIC / 'logo512.png')

# Check encoded dimensions and retain transparent corners at every size.
for path in list(ICONS.glob('*.png')) + [PUBLIC / 'logo192.png', PUBLIC / 'logo512.png']:
    with Image.open(path) as image:
        assert image.mode == 'RGBA', path
        assert image.getpixel((0,0))[3] == 0, path
with Image.open(ICONS / 'icon.ico') as image:
    assert image.size == (256,256)
    assert image.ico.sizes() == {(size,size) for size in sizes}
with Image.open(ICONS / 'icon.icns') as image:
    assert image.format == 'ICNS'
print('Generated and verified Windows, macOS and web icons from C: 浮字金印.')
