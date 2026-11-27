---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 25 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person f4ca2772 <user-85d07053d5@example.invalid>
Subject: [PATCH] inetutils: patch CVE-2026-95510

From: Person f4ca2772 <user-85d07053d5@example.invalid>

Pick patch referencing this CVE.

Signed-off-by: Person f4ca2772 <user-85d07053d5@example.invalid>
---
 .../inetutils/inetutils/CVE-2026-95510.patch  | 40 +++++++++++++++++++
 .../inetutils/inetutils_2.8.bb                |  1 +
 2 files changed, 41 insertions(+)
 create mode 100644 meta/recipes-connectivity/inetutils/inetutils/CVE-2026-95510.patch

diff --git a/meta/recipes-connectivity/inetutils/inetutils/CVE-2026-95510.patch b/meta/recipes-connectivity/inetutils/inetutils/CVE-2026-95510.patch
new file mode 100644
index 00000000000..9160b0b618c
--- /dev/null
+++ b/meta/recipes-connectivity/inetutils/inetutils/CVE-2026-95510.patch
@@ -0,0 +1,40 @@
+From f756321da9b964fc27fc43652b73913117883047 Mon Sep 17 00:00:00 2001
+From: Person 8273be3e <user-d82ce0353b@example.invalid>
+Date: Mon, 14 Sep 2026 15:44:23 -0700
+Subject: [PATCH] libinetutils: zero-initialize sigactions and sigvecs
+
+This commit fixes CVE-2026-95510.
+
+* libinetutils/setsig.c (setsig): Initialize the siactions and sigvecs
+to zero.
+Copyright-paperwork-exempt: Yes
+
+CVE: CVE-2026-95510
+Upstream-Status: Backport [https://cgit.git.savannah.gnu.org/cgit/inetutils.git/commit/?id=f756321da9b964fc27fc43652b73913117883047]
+Signed-off-by: Person f4ca2772 <user-85d07053d5@example.invalid>
+---
+ libinetutils/setsig.c | 4 ++--
+ 1 file changed, 2 insertions(+), 2 deletions(-)
+
+diff --git a/libinetutils/setsig.c b/libinetutils/setsig.c
+index de7fa7a5..29c6980e 100644
+--- a/libinetutils/setsig.c
++++ b/libinetutils/setsig.c
+@@ -30,7 +30,7 @@ sighandler_t
+ setsig (int sig, sighandler_t handler)
+ {
+ #ifdef HAVE_SIGACTION
+-  struct sigaction sa, osa;
++  struct sigaction sa = { 0 }, osa = { 0 };
+   sigemptyset (&sa.sa_mask);
+   sigemptyset (&osa.sa_mask);
+ # ifdef SA_RESTART
+@@ -42,7 +42,7 @@ setsig (int sig, sighandler_t handler)
+   return osa.sa_handler;
+ #else /* !HAVE_SIGACTION */
+ # ifdef HAVE_SIGVEC
+-  struct sigvec sv, osv;
++  struct sigvec sv = { 0 }, osv = { 0 };
+   sigemptyset (&sv.sv_mask);
+   sigemptyset (&osv.sv_mask);
+   sv.sv_handler = handler;
diff --git a/meta/recipes-connectivity/inetutils/inetutils_2.8.bb b/meta/recipes-connectivity/inetutils/inetutils_2.8.bb
index b87c0ede43b..982940e4958 100644
--- a/meta/recipes-connectivity/inetutils/inetutils_2.8.bb
+++ b/meta/recipes-connectivity/inetutils/inetutils_2.8.bb
@@ -18,6 +18,7 @@ SRC_URI = "${GNU_MIRROR}/inetutils/inetutils-${PV}.tar.gz \
            file://rsh.xinetd.inetutils \
            file://telnet.xinetd.inetutils \
            file://tftpd.xinetd.inetutils \
+           file://CVE-2026-95510.patch \
            "
 
 inherit autotools gettext update-alternatives texinfo
````
