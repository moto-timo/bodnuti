---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [real]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person 05156f3e <user-74a2de9008@example.invalid>
Subject: [meta-python][scarthgap][PATCH 2/2] python3-pillow: Fix CVE-2026-55380

This patch applies the upstream fix as referenced in [2],
using the commit shown in [1].

[1] https://github.com/python-pillow/Pillow/commit/f39b0ae6624eb2d7c5c5d651d9bb5fdbd96a8675
[2] https://github.com/python-pillow/Pillow/security/advisories/GHSA-phj9-mv4w-65pm

Signed-off-by: Person 05156f3e <user-74a2de9008@example.invalid>
---
 .../python3-pillow/CVE-2026-55380.patch       | 66 +++++++++++++++++++
 .../python/python3-pillow_10.3.0.bb           |  1 +
 2 files changed, 67 insertions(+)
 create mode 100644 meta-python/recipes-devtools/python/python3-pillow/CVE-2026-55380.patch

diff --git a/meta-python/recipes-devtools/python/python3-pillow/CVE-2026-55380.patch b/meta-python/recipes-devtools/python/python3-pillow/CVE-2026-55380.patch
new file mode 100644
index 0000000000..0c4ef9658c
--- /dev/null
+++ b/meta-python/recipes-devtools/python/python3-pillow/CVE-2026-55380.patch
@@ -0,0 +1,66 @@
+From 176781ac99ebf9aa5486384e6e2e4896b30d95b5 Mon Sep 17 00:00:00 2001
+From: Person d5617a4a <user-bc340b9f86@example.invalid>
+Date: Sun, 21 Jun 2026 14:06:24 +1000
+Subject: [PATCH] Add decompression bomb check to GdImageFile (#9693)
+
+CVE: CVE-2026-55380
+Upstream-Status: Backport [https://github.com/python-pillow/Pillow/commit/f39b0ae6624eb2d7c5c5d651d9bb5fdbd96a8675]
+
+(cherry picked from commit f39b0ae6624eb2d7c5c5d651d9bb5fdbd96a8675)
+
+Signed-off-by: Person 05156f3e <user-74a2de9008@example.invalid>
+---
+ Tests/test_file_gd.py  | 8 +++++++-
+ src/PIL/GdImageFile.py | 3 ++-
+ 2 files changed, 9 insertions(+), 2 deletions(-)
+
+diff --git a/Tests/test_file_gd.py b/Tests/test_file_gd.py
+index d512df284..3db71463c 100644
+--- a/Tests/test_file_gd.py
++++ b/Tests/test_file_gd.py
+@@ -2,7 +2,7 @@ from __future__ import annotations
+ 
+ import pytest
+ 
+-from PIL import GdImageFile, UnidentifiedImageError
++from PIL import GdImageFile, Image, UnidentifiedImageError, _binary
+ 
+ TEST_GD_FILE = "Tests/images/hopper.gd"
+ 
+@@ -18,6 +18,12 @@ def test_bad_mode() -> None:
+         GdImageFile.open(TEST_GD_FILE, "bad mode")
+ 
+ 
++def test_decompression_bomb() -> None:
++    b = BytesIO(_binary.o16be(65535) + _binary.o16be(50000) + _binary.o16be(50000))
++    with pytest.raises(Image.DecompressionBombError):
++        GdImageFile.open(b)
++
++
+ def test_invalid_file() -> None:
+     invalid_file = "Tests/images/flower.jpg"
+ 
+diff --git a/src/PIL/GdImageFile.py b/src/PIL/GdImageFile.py
+index 88b87a22c..9dd79de3f 100644
+--- a/src/PIL/GdImageFile.py
++++ b/src/PIL/GdImageFile.py
+@@ -29,7 +29,7 @@ from __future__ import annotations
+ 
+ from typing import IO
+ 
+-from . import ImageFile, ImagePalette, UnidentifiedImageError
++from . import Image, ImageFile, ImagePalette, UnidentifiedImageError
+ from ._binary import i16be as i16
+ from ._binary import i32be as i32
+ from ._typing import StrOrBytesPath
+@@ -58,6 +58,7 @@ class GdImageFile(ImageFile.ImageFile):
+ 
+         self._mode = "L"  # FIXME: "P"
+         self._size = i16(s, 2), i16(s, 4)
++        Image._decompression_bomb_check(self.size)
+ 
+         true_color = s[6]
+         true_color_offset = 2 if true_color else 0
+-- 
+2.50.1
+
diff --git a/meta-python/recipes-devtools/python/python3-pillow_10.3.0.bb b/meta-python/recipes-devtools/python/python3-pillow_10.3.0.bb
index 55df58ef4b..f27bcac7d2 100644
--- a/meta-python/recipes-devtools/python/python3-pillow_10.3.0.bb
+++ b/meta-python/recipes-devtools/python/python3-pillow_10.3.0.bb
@@ -15,6 +15,7 @@ SRC_URI = "git://github.com/python-pillow/Pillow.git;branch=main;protocol=https
            file://CVE-2026-59204.patch \
            file://CVE-2026-112393.patch \
            file://CVE-2026-54059-CVE-2026-54060-CVE-2026-55379.patch \
+           file://CVE-2026-55380.patch \
            "
 SRCREV = "5c89d88eee199ba53f64581ea39b6a1bc52feb1a"
 
-- 
2.50.1



````
