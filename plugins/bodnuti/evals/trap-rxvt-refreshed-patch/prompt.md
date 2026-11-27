---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person 97283abd <user-e37ee0b62d@example.invalid>
Subject: [PATCH 10/11] rxvt-unicode: upgrade 9.31 -> 9.32

Changes: https://cvs.schmorp.de/rxvt-unicode/Changes

9.32 Tue 10 Nov 2026 04:56:52 CET
- NOTICE: due to unforseen circumstances, we could not get a proper fix for
          the cursor positon on resize issues, but decided to release anyway for
          the
          other bugfixes.
- fix incorrect array bounds check when parsing OSC/DCS (reported by Tristan).
        - add DECSCUSR 7 for the "power-up" configuration, similar to xterm.
        - mbrtowc can decode NOCHAR, which collides with urxvt's internal use,
          potentially freezing the terminal (Mikachu).
        - fix memory leak when a background expression uses the muladd
          or focus_fade operators (reported by Erik-Jan).
        - fix reference counting of values returned by some perl bindings.
        - fix OSC responses with 7-bit ST (reported by Person bd810495).
        - fix mouse reporting of motion notify events to report 3 as
          the button number when no button is pressed, same as xterm.
          Previously urxvt reported the number of the last button
          pressed or an invalid value, before the first button press
          (reported by Person d04ebcab).
        - change the encoding of buttons 8-11 in mouse reporting
          sequences to match xterm's and enable reporting of release
          events for them (patch by Person 0ccacac5).
        - clamp coordinates in mouse reporting sequences to the number
          of rows and columns (patch by Person e790ab50).
- added Alt_L/ALt_R and updated some keys in iso 14755 5.2 mode.
        - perl 5.38 does not handle empty locale names, use "C" instead
          (patch by sxzzsf).
        - add emoji test character, so that emoji-only fonts can be used without
          any tricks.
        - urxvt assumes every font has space, but the built-in font did not have
          one,
          causing occasional rectangles in tetriscurses (analyzed by Mikachu's
          LLM
          website).
        - the confirm-paste extension now offers a choice between pasting the
          original
        - this version has been verified to work with perl 5.8.3.

Signed-off-by: Person 97283abd <user-e37ee0b62d@example.invalid>
---
 .../0001-rxvt-unicode-fix-gcc16-c-20.patch    |  5 +----
 .../rxvt-unicode/rxvt-unicode/xwc.patch       | 19 ++++++++++++++-----
 ...t-unicode_9.31.bb => rxvt-unicode_9.32.bb} |  2 +-
 3 files changed, 16 insertions(+), 10 deletions(-)
 rename meta/recipes-sato/rxvt-unicode/{rxvt-unicode_9.31.bb => rxvt-unicode_9.32.bb} (71%)

diff --git a/meta/recipes-sato/rxvt-unicode/rxvt-unicode/0001-rxvt-unicode-fix-gcc16-c-20.patch b/meta/recipes-sato/rxvt-unicode/rxvt-unicode/0001-rxvt-unicode-fix-gcc16-c-20.patch
index facb574a2b..2c463ecc28 100644
--- a/meta/recipes-sato/rxvt-unicode/rxvt-unicode/0001-rxvt-unicode-fix-gcc16-c-20.patch
+++ b/meta/recipes-sato/rxvt-unicode/rxvt-unicode/0001-rxvt-unicode-fix-gcc16-c-20.patch
@@ -1,4 +1,4 @@
-From 4ff364c3df99b02e658ac2b3d2eec7139c494612 Mon Sep 17 00:00:00 2001
+From 9d292205d6966d19ceaae10fe021145ca6ee8116 Mon Sep 17 00:00:00 2001
 From: Person 2bb59e67 <user-3a61bd7f6c@example.invalid>
 Date: Tue, 28 Apr 2026 03:22:40 -0700
 Subject: [PATCH] rxvt-unicode: fix gcc16/c++20
@@ -56,6 +56,3 @@ index d08b1bb..ec69cc1 100644
  {
    return (long(a) * long(100 - p) + long(b) * long(p) + 50) / 100;
  }
--- 
-2.49.0
-
diff --git a/meta/recipes-sato/rxvt-unicode/rxvt-unicode/xwc.patch b/meta/recipes-sato/rxvt-unicode/rxvt-unicode/xwc.patch
index 16efeef49f..dd11db6a2c 100644
--- a/meta/recipes-sato/rxvt-unicode/rxvt-unicode/xwc.patch
+++ b/meta/recipes-sato/rxvt-unicode/rxvt-unicode/xwc.patch
@@ -1,9 +1,18 @@
+From 74481f4ed205ab215f8dd467263776ca4f9531c0 Mon Sep 17 00:00:00 2001
+From: Person d24178b3 <user-ffb2b9bbc5@example.invalid>
+Date: Tue, 19 Apr 2011 14:58:37 +0800
+Subject: [PATCH] rxvt-unicode: upgrade to version 9.10
+
 Upstream-Status: Pending
+---
+ src/screen.C | 4 +++-
+ 1 file changed, 3 insertions(+), 1 deletion(-)
 
-diff -ruN rxvt-unicode-9.10-orig//src/screen.C rxvt-unicode-9.10/src/screen.C
---- rxvt-unicode-9.10-orig//src/screen.C	2011-04-19 13:34:01.327664984 +0800
-+++ rxvt-unicode-9.10/src/screen.C	2011-04-19 13:34:47.037665001 +0800
-@@ -3152,7 +3152,7 @@
+diff --git a/src/screen.C b/src/screen.C
+index 8fdfad9..9a9e3b5 100644
+--- a/src/screen.C
++++ b/src/screen.C
+@@ -2941,7 +2941,7 @@ rxvt_term::selection_grab (Time tm, bool clipboard) noexcept
        return false;
      }
  
@@ -12,7 +21,7 @@ diff -ruN rxvt-unicode-9.10-orig//src/screen.C rxvt-unicode-9.10/src/screen.C
    XTextProperty ct;
  
    if (XwcTextListToTextProperty (dpy, &selection.text, 1, XStringStyle, &ct) >= 0)
-@@ -3734,9 +3734,11 @@
+@@ -3524,9 +3524,11 @@ rxvt_term::selection_send (const XSelectionRequestEvent &rq) noexcept
          }
        else
  #endif
diff --git a/meta/recipes-sato/rxvt-unicode/rxvt-unicode_9.31.bb b/meta/recipes-sato/rxvt-unicode/rxvt-unicode_9.32.bb
similarity index 71%
rename from meta/recipes-sato/rxvt-unicode/rxvt-unicode_9.31.bb
rename to meta/recipes-sato/rxvt-unicode/rxvt-unicode_9.32.bb
index c127b9bbe3..42b480c4cb 100644
--- a/meta/recipes-sato/rxvt-unicode/rxvt-unicode_9.31.bb
+++ b/meta/recipes-sato/rxvt-unicode/rxvt-unicode_9.32.bb
@@ -4,5 +4,5 @@ LICENSE = "GPL-3.0-only"
 LIC_FILES_CHKSUM = "file://COPYING;md5=d32239bcb673463ab874e80d47fae504 \
                     file://src/main.C;beginline=1;endline=31;md5=d3600d7ee1062667fcd1193fbe6485f6"
 
-SRC_URI[sha256sum] = "aaa13fcbc149fe0f3f391f933279580f74a96fd312d6ed06b8ff03c2d46672e8"
+SRC_URI[sha256sum] = "2f81c16bffb16f97c4633032e08a7ef08c000f81eb05853e950febc6c33328dd"
 

````
