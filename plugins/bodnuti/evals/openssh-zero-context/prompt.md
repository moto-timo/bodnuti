---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [real]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: "Person 3fb710cd -X (hthakar - E INFOCHIPS PRIVATE LIMITED at Cisco)" <user-103181c8e3@example.invalid>
Subject: [OE-core][wrynose][PATCH 08/10] openssh: Fix CVE-2026-106583

From: Person 3fb710cd <user-103181c8e3@example.invalid>

This patch applies the upstream fix as referenced in [2], using the
commit shown in [1]. The two other commits linked by [2] only add and
adjust regression tests; they contain no functional security fix.

[1] https://github.com/openssh/openssh-portable/commit/0c4c9a7b320bd7ff61c4a2dc345cb2e27e9663b2
[2] https://ubuntu.com/security/CVE-2026-106583

Signed-off-by: Person 3fb710cd <user-103181c8e3@example.invalid>
---
 .../openssh/openssh/CVE-2026-106583.patch     | 35 +++++++++++++++++++
 .../openssh/openssh_10.3p1.bb                 |  1 +
 2 files changed, 36 insertions(+)
 create mode 100644 meta/recipes-connectivity/openssh/openssh/CVE-2026-106583.patch

diff --git a/meta/recipes-connectivity/openssh/openssh/CVE-2026-106583.patch b/meta/recipes-connectivity/openssh/openssh/CVE-2026-106583.patch
new file mode 100644
index 0000000000..094f4bad63
--- /dev/null
+++ b/meta/recipes-connectivity/openssh/openssh/CVE-2026-106583.patch
@@ -0,0 +1,35 @@
+From 0c4c9a7b320bd7ff61c4a2dc345cb2e27e9663b2 Mon Sep 17 00:00:00 2001
+From: "user-1e89504bba@example.invalid" <user-1e89504bba@example.invalid>
+Date: Mon, 5 Oct 2026 06:03:40 +0000
+Subject: [PATCH] upstream: Further restrict the characters allowed in a command-line
+
+supplied user name, disallowing '$' and '\'. Reported by SecBuddyF KeenLab
+Tencent (CodeBuddy Security).
+
+OpenBSD-Commit-ID: 13671c178f492af63b5c1296f284818c7809ed9a
+
+CVE: CVE-2026-106583
+Upstream-Status: Backport [https://github.com/openssh/openssh-portable/commit/0c4c9a7b320bd7ff61c4a2dc345cb2e27e9663b2]
+
+Backport Changes:
+- Omitted the upstream readconf.c OpenBSD revision-only hunk and retained
+  the Wrynose OpenSSH 10.3p1 revision.
+
+(cherry picked from commit 0c4c9a7b320bd7ff61c4a2dc345cb2e27e9663b2)
+Signed-off-by: Person 3fb710cd <user-103181c8e3@example.invalid>
+---
+ readconf.c | 5 +----
+ 1 file changed, 1 insertion(+), 4 deletions(-)
+
+diff --git a/readconf.c b/readconf.c
+--- a/readconf.c
++++ b/readconf.c
+@@ -3452 +3452 @@ ssh_valid_ruser(const char *s)
+-		if (strchr("'`\";&<>|(){}", s[i]) != NULL)
++		if (strchr("'`\";&<>|(){}$\\", s[i]) != NULL)
+@@ -3457,3 +3457,0 @@ ssh_valid_ruser(const char *s)
+-		/* Disallow \ in last position */
+-		if (s[i] == '\\' && s[i + 1] == '\0')
+-			return 0;
+-- 
+2.43.0
diff --git a/meta/recipes-connectivity/openssh/openssh_10.3p1.bb b/meta/recipes-connectivity/openssh/openssh_10.3p1.bb
index 44d9881815..8545868d81 100644
--- a/meta/recipes-connectivity/openssh/openssh_10.3p1.bb
+++ b/meta/recipes-connectivity/openssh/openssh_10.3p1.bb
@@ -42,6 +42,7 @@ SRC_URI = "https://ftp.openbsd.org/pub/OpenBSD/OpenSSH/portable/openssh-${PV}.ta
            file://CVE-2026-106587.patch \
            file://CVE-2026-106582.patch \
            file://CVE-2026-106585.patch \
+           file://CVE-2026-106583.patch \
            "
 SRC_URI[sha256sum] = "56682a36bb92dcf4b4f016fd8ec8e74059b79a8de25c15d670d731e7d18e45f4"
 
-- 
2.35.6


````
