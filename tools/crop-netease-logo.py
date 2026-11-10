"""从用户截图里裁出网易云的「图标 + 文字」横版标志（以及单独的圆形图标）。

截图里的定位方式：
* 红色圆标 —— 连通域里面积最大、填充率高、近方形的那块；
* 「网易云音乐」白字 —— 同一水平带上的高亮度、低彩度像素，按列分成 5 个字符块；
* 两者取并集得到横版 lockup 的包围盒。

坑：像素运算必须用 float/int32 —— 在 uint8/int16 上做 `x * 255` 会溢出成负数，
抠图结果会变成整张全透明（第一版就是这么翻车的）。
"""
from collections import deque
from PIL import Image, ImageFilter
import numpy as np
import sys

if len(sys.argv) < 5:
    raise SystemExit("用法: crop-netease-logo.py <src> <out-lockup> <out-icon> <out-preview>")

src, out_lockup, out_icon, out_preview = sys.argv[1:5]

im = Image.open(src).convert("RGBA")
a = np.asarray(im).astype(np.int32)
r, g, b = a[..., 0], a[..., 1], a[..., 2]
h, w = r.shape


def biggest_round(red_mask):
    """连通域里挑最像实心圆的一块"""
    seen = np.zeros_like(red_mask, dtype=bool)
    best = None
    for y in range(h):
        for x in range(w):
            if not red_mask[y, x] or seen[y, x]:
                continue
            q = deque([(y, x)])
            seen[y, x] = True
            pix = []
            while q:
                cy, cx = q.popleft()
                pix.append((cy, cx))
                for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    ny, nx = cy + dy, cx + dx
                    if 0 <= ny < h and 0 <= nx < w and red_mask[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        q.append((ny, nx))
            ys = [p[0] for p in pix]
            xs = [p[1] for p in pix]
            bw, bh = max(xs) - min(xs) + 1, max(ys) - min(ys) + 1
            area = len(pix)
            fill = area / (bw * bh)
            square = min(bw, bh) / max(bw, bh)
            if fill > 0.6 and square > 0.85 and area > 200:
                if best is None or area > best[0]:
                    best = (area, min(xs), max(xs), min(ys), max(ys))
    return best


red = (r > 90) & (r - g > 40) & (r - b > 40)
icon = biggest_round(red)
if icon is None:
    raise SystemExit("没找到圆形红色标记")
_, ix0, ix1, iy0, iy1 = icon
print(f"圆形图标 x{ix0}-{ix1} y{iy0}-{iy1}（{ix1-ix0+1}x{iy1-iy0+1}）")

# ---- 文字：与图标同一水平带的高亮度低彩度像素 ----
mx = np.maximum(np.maximum(r, g), b)
whiteish = (mx > 150) & (np.abs(r - g) < 45) & (np.abs(g - b) < 45)
band = np.zeros_like(whiteish, dtype=bool)
band[max(0, iy0 - 2): min(h, iy1 + 3), :] = True
text = whiteish & band
text[:, : ix1 + 3] = False          # 排除圆标内部那个白色音符

ys, xs = np.nonzero(text)
if len(xs) == 0:
    raise SystemExit("没找到「网易云音乐」文字")
tx0, tx1, ty0, ty1 = xs.min(), xs.max(), ys.min(), ys.max()
print(f"文字 x{tx0}-{tx1} y{ty0}-{ty1}（{tx1-tx0+1}x{ty1-ty0+1}）")

pad = 2
bx0, bx1 = max(0, min(ix0, tx0) - pad), min(w, max(ix1, tx1) + 1 + pad)
by0, by1 = max(0, min(iy0, ty0) - pad), min(h, max(iy1, ty1) + 1 + pad)
print(f"横向 lockup 包围盒 x{bx0}-{bx1} y{by0}-{by1}（{bx1-bx0}x{by1-by0}）")


def key_out(box, target_h):
    """裁切 + 黑底转透明 + 按高度等比缩放"""
    crop = im.crop(box)
    c = np.asarray(crop).astype(np.float32)
    mx2 = c[..., :3].max(axis=2)
    alpha = np.clip((mx2 - 26.0) * 255.0 / 180.0, 0.0, 255.0)
    out = crop.copy()
    out.putalpha(Image.fromarray(alpha.astype(np.uint8)))
    scale = target_h / crop.height
    size = (max(1, round(crop.width * scale)), target_h)
    return out.resize(size, Image.LANCZOS).filter(
        ImageFilter.UnsharpMask(radius=1.1, percent=60, threshold=2)
    )


lockup = key_out((bx0, by0, bx1, by1), 44)      # 44px 高 = 界面 22px 的 2 倍
lockup.save(out_lockup)
print(f"横版 lockup -> {out_lockup} ({lockup.width}x{lockup.height})")

round_icon = key_out((max(0, ix0 - pad), max(0, iy0 - pad),
                      min(w, ix1 + 1 + pad), min(h, iy1 + 1 + pad)), 96)
round_icon.save(out_icon)
print(f"圆形图标 -> {out_icon} ({round_icon.width}x{round_icon.height})")

preview = Image.new("RGBA", (lockup.width + 40, lockup.height + 40), (26, 26, 30, 255))
preview.alpha_composite(lockup, (20, 20))
preview.save(out_preview)
print(f"预览 -> {out_preview}")
