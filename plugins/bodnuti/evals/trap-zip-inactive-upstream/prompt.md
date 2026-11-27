---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person f4ca2772 <user-85d07053d5@example.invalid>
Subject: [scarthgap][PATCH 1/3] zip: fix a buffer overflow detected issue

From: Person a8d99b5d <user-2443d29a51@example.invalid>

A "buffer overflow detected" issue was observed as follows:
| *** buffer overflow detected ***: terminated
|
| zip error: Interrupted (aborting)

This issue is addressed by:
https://bugzilla.redhat.com/show_bug.cgi?id=2165653

Port the fix.

Signed-off-by: Person a8d99b5d <user-2443d29a51@example.invalid>
Signed-off-by: Person 8ccd633f <user-0e9880ba7f@example.invalid>
(From OE-Core rev: e2f3eeaedc0ea896f5f5b23f756056331b1647cf)
Signed-off-by: Person f4ca2772 <user-85d07053d5@example.invalid>
---
 ...fix-a-buffer-overflow-detected-issue.patch | 38 +++++++++++++++++++
 meta/recipes-extended/zip/zip_3.0.bb          |  1 +
 2 files changed, 39 insertions(+)
 create mode 100644 meta/recipes-extended/zip/zip-3.0/0001-fileio.c-fix-a-buffer-overflow-detected-issue.patch

diff --git a/meta/recipes-extended/zip/zip-3.0/0001-fileio.c-fix-a-buffer-overflow-detected-issue.patch b/meta/recipes-extended/zip/zip-3.0/0001-fileio.c-fix-a-buffer-overflow-detected-issue.patch
new file mode 100644
index 00000000000..d760dfcec5a
--- /dev/null
+++ b/meta/recipes-extended/zip/zip-3.0/0001-fileio.c-fix-a-buffer-overflow-detected-issue.patch
@@ -0,0 +1,38 @@
+From 23b4ed82bff20c737fe2e95d5b035e92a9522ca2 Mon Sep 17 00:00:00 2001
+From: Person a8d99b5d <user-2443d29a51@example.invalid>
+Date: Thu, 15 May 2025 13:58:45 +0200
+Subject: [PATCH] fileio.c: fix a buffer overflow detected issue
+MIME-Version: 1.0
+Content-Type: text/plain; charset=UTF-8
+Content-Transfer-Encoding: 8bit
+
+Fix a following issue:
+| *** buffer overflow detected ***: terminated
+|
+| zip error: Interrupted (aborting)
+
+Reference: https://bugzilla.redhat.com/show_bug.cgi?id=2165653
+
+Upstream-Status: Inactive-Upstream [the fix is from Redhat but not the official project]
+
+Signed-off-by: Person a8d99b5d <user-2443d29a51@example.invalid>
+---
+ fileio.c | 2 +-
+ 1 file changed, 1 insertion(+), 1 deletion(-)
+
+diff --git a/fileio.c b/fileio.c
+index 1847e62..5a2959d 100644
+--- a/fileio.c
++++ b/fileio.c
+@@ -3502,7 +3502,7 @@ zwchar *local_to_wide_string(local_string)
+   if ((wc_string = (wchar_t *)malloc((wsize + 1) * sizeof(wchar_t))) == NULL) {
+     ZIPERR(ZE_MEM, "local_to_wide_string");
+   }
+-  wsize = mbstowcs(wc_string, local_string, strlen(local_string) + 1);
++  wsize = mbstowcs(wc_string, local_string, wsize + 1);
+   wc_string[wsize] = (wchar_t) 0;
+ 
+   /* in case wchar_t is not zwchar */
+-- 
+2.43.0
+
diff --git a/meta/recipes-extended/zip/zip_3.0.bb b/meta/recipes-extended/zip/zip_3.0.bb
index 58144baa149..9ef9a9074f9 100644
--- a/meta/recipes-extended/zip/zip_3.0.bb
+++ b/meta/recipes-extended/zip/zip_3.0.bb
@@ -19,6 +19,7 @@ SRC_URI = "${SOURCEFORGE_MIRROR}/infozip/Zip%203.x%20%28latest%29/3.0/zip30.tar.
            file://0001-configure-Specify-correct-function-signatures-and-de.patch \
            file://0001-unix-configure-use-_Static_assert-to-do-correct-dete.patch \
            file://0001-configure-Include-dirent.h-for-closedir-opendir-APIs.patch \
+           file://0001-fileio.c-fix-a-buffer-overflow-detected-issue.patch \
            "
 UPSTREAM_VERSION_UNKNOWN = "1"
 

````
