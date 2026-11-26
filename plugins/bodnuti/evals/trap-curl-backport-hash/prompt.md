---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person 3b9c120b <user-cca8b48fd2@example.invalid>
Subject: [blacksail][PATCH v2 3/4] curl: fix CVE-2026-92391

This vulnerability, marked as High [1], was fixed upstream [2] since
v8.23.0.

Backport the fix to curl v8.22.0.

[1] https://nvd.nist.gov/vuln/detail/cve-2026-92391
[2] https://curl.se/docs/CVE-2026-92391.html

Signed-off-by: Person 2135b8da <user-cca8b48fd2@example.invalid>
---
 .../curl/curl/CVE-2026-92391.patch            | 200 ++++++++++++++++++
 meta/recipes-support/curl/curl_8.22.0.bb      |   1 +
 2 files changed, 201 insertions(+)
 create mode 100644 meta/recipes-support/curl/curl/CVE-2026-92391.patch

diff --git a/meta/recipes-support/curl/curl/CVE-2026-92391.patch b/meta/recipes-support/curl/curl/CVE-2026-92391.patch
new file mode 100644
index 0000000000..80ab126ee7
--- /dev/null
+++ b/meta/recipes-support/curl/curl/CVE-2026-92391.patch
@@ -0,0 +1,200 @@
+From e1926bb54733cc768d31c4153720f6fb7d927706 Mon Sep 17 00:00:00 2001
+From: Person 0ec3d4b9 <user-69fdca87ed@example.invalid>
+Date: Sun, 13 Sep 2026 00:06:44 +0200
+Subject: [PATCH] mime: do curl_mime_free() non-recursively
+MIME-Version: 1.0
+Content-Type: text/plain; charset=UTF-8
+Content-Transfer-Encoding: 8bit
+
+Verify with test 3595: deeply nested curl_mime_free()
+
+Closes #22927
+
+CVE: CVE-2026-92391
+Upstream-Status: Backport [https://github.com/curl/curl/commit/d7546809d5bd548b69ec49fca26565c8f6f0d344]
+
+Signed-off-by: Person 2135b8da <user-cca8b48fd2@example.invalid>
+---
+ lib/mime.c                 | 28 +++++++++++----
+ tests/data/Makefile.am     |  2 +-
+ tests/data/test3595        | 21 ++++++++++++
+ tests/libtest/Makefile.inc |  1 +
+ tests/libtest/lib3595.c    | 70 ++++++++++++++++++++++++++++++++++++++
+ 5 files changed, 115 insertions(+), 7 deletions(-)
+ create mode 100644 tests/data/test3595
+ create mode 100644 tests/libtest/lib3595.c
+
+diff --git a/lib/mime.c b/lib/mime.c
+index 6004375..41150dc 100644
+--- a/lib/mime.c
++++ b/lib/mime.c
+@@ -1109,20 +1109,36 @@ void Curl_mime_cleanpart(curl_mimepart *part)
+   }
+ }
+ 
+-/* Recursively delete a mime handle and its parts. */
++/* Non-recursively delete a mime handle and its parts. */
+ void curl_mime_free(curl_mime *mime)
+ {
+   curl_mimepart *part;
+ 
+-  if(mime) {
+-    mime_subparts_unbind(mime);  /* Be sure it is not referenced anymore. */
+-    while(mime->firstpart) {
+-      part = mime->firstpart;
++  if(!mime)
++    return;
++
++  mime_subparts_unbind(mime);  /* Be sure it is not referenced anymore. */
++
++  while(mime) {
++    part = mime->firstpart;
++    if(part) {
+       mime->firstpart = part->nextpart;
++      if(part->kind == MIMEKIND_MULTIPART && part->arg &&
++         part->freefunc == mime_subparts_free) {
++        curl_mime *subparts = (curl_mime *)part->arg;
++        part->freefunc = NULL;
++        cleanup_part_content(part);
++        subparts->parent = mime->parent;
++        mime->parent = (curl_mimepart *)subparts;
++      }
+       Curl_mime_cleanpart(part);
+       curlx_free(part);
+     }
+-    curlx_free(mime);
++    else {
++      curl_mime *parent = (curl_mime *)mime->parent;
++      curlx_free(mime);
++      mime = parent;
++    }
+   }
+ }
+ 
+diff --git a/tests/data/Makefile.am b/tests/data/Makefile.am
+index 4a27927..e0ce23a 100644
+--- a/tests/data/Makefile.am
++++ b/tests/data/Makefile.am
+@@ -2091,7 +2091,7 @@ TESTCASES = \
+   test3401 \
+   \
+   test3500 \
+-  \
++  test3595 \
+   test4000 \
+   test4001 \
+   \
+diff --git a/tests/data/test3595 b/tests/data/test3595
+new file mode 100644
+index 0000000..b2b1dd2
+--- /dev/null
++++ b/tests/data/test3595
+@@ -0,0 +1,21 @@
++<?xml version="1.0" encoding="US-ASCII"?>
++<testcase>
++<info>
++<keywords>
++mime
++</keywords>
++</info>
++
++# Client-side
++<client>
++<tool>
++lib%TESTNUMBER
++</tool>
++<name>
++curl_mime_free
++</name>
++</client>
++
++<verify>
++</verify>
++</testcase>
+diff --git a/tests/libtest/Makefile.inc b/tests/libtest/Makefile.inc
+index 26c72e5..3d25a83 100644
+--- a/tests/libtest/Makefile.inc
++++ b/tests/libtest/Makefile.inc
+@@ -325,5 +325,6 @@ TESTS_C = \
+   lib3105.c \
+   lib3207.c \
+   lib3208.c \
++  lib3595.c \
+   lib5000.c \
+   lib5004.c
+diff --git a/tests/libtest/lib3595.c b/tests/libtest/lib3595.c
+new file mode 100644
+index 0000000..1f343f5
+--- /dev/null
++++ b/tests/libtest/lib3595.c
+@@ -0,0 +1,70 @@
++/***************************************************************************
++ *                                  _   _ ____  _
++ *  Project                     ___| | | |  _ \| |
++ *                             / __| | | | |_) | |
++ *                            | (__| |_| |  _ <| |___
++ *                             \___|\___/|_| \_\_____|
++ *
++ * Copyright (C) Person 0ec3d4b9, <user-69fdca87ed@example.invalid>, et al.
++ *
++ * This software is licensed as described in the file COPYING, which
++ * you should have received as part of this distribution. The terms
++ * are also available at https://curl.se/docs/copyright.html.
++ *
++ * You may opt to use, copy, modify, merge, publish, distribute and/or sell
++ * copies of the Software, and permit persons to whom the Software is
++ * furnished to do so, under the terms of the COPYING file.
++ *
++ * This software is distributed on an "AS IS" basis, WITHOUT WARRANTY OF ANY
++ * KIND, either express or implied.
++ *
++ * SPDX-License-Identifier: curl
++ *
++ ***************************************************************************/
++#include "unitcheck.h"
++
++static CURLcode test_lib3595(const char *arg)
++{
++  UNITTEST_BEGIN_SIMPLE
++
++#if !defined(CURL_DISABLE_MIME) &&                                      \
++  (!defined(CURL_DISABLE_HTTP) ||                                       \
++   !defined(CURL_DISABLE_SMTP) ||                                       \
++   !defined(CURL_DISABLE_IMAP))
++
++  curl_mime *root = NULL;
++  curl_mime *current = NULL;
++  curl_mime *sub = NULL;
++  curl_mimepart *part = NULL;
++  int i;
++
++  if(curl_global_init(CURL_GLOBAL_ALL) != CURLE_OK) {
++    curl_mfprintf(stderr, "curl_global_init() failed\n");
++    return TEST_ERR_MAJOR_BAD;
++  }
++
++  /* Create a deeply nested mime structure to verify that non-recursive
++     curl_mime_free works properly. */
++  (void)arg;
++  root = curl_mime_init(NULL);
++  fail_unless(root != NULL, "curl_mime_init root failed");
++  current = root;
++
++  for(i = 0; i < 50; i++) {
++    part = curl_mime_addpart(current);
++    fail_unless(part != NULL, "curl_mime_addpart failed");
++    sub = curl_mime_init(NULL);
++    fail_unless(sub != NULL, "curl_mime_init sub failed");
++    curl_mime_subparts(part, sub);
++    current = sub;
++  }
++
++  /* Free deep structure */
++  curl_mime_free(root);
++  curl_global_cleanup();
++#else
++  (void)arg;
++#endif
++
++  UNITTEST_END_SIMPLE
++}
diff --git a/meta/recipes-support/curl/curl_8.22.0.bb b/meta/recipes-support/curl/curl_8.22.0.bb
index 4c6a088e62..6445aae426 100644
--- a/meta/recipes-support/curl/curl_8.22.0.bb
+++ b/meta/recipes-support/curl/curl_8.22.0.bb
@@ -19,6 +19,7 @@ SRC_URI = " \
     file://CVE-2026-92389.patch \
     file://CVE-2026-87865.patch \
     file://CVE-2026-92386.patch \
+    file://CVE-2026-92391.patch \
 "
 
 SRC_URI:append:class-nativesdk = " \
-- 
2.55.0


````
