---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 25 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person f4ca2772 <user-85d07053d5@example.invalid>
Subject: [PATCH] busybox: patch CVE-2026-136411

From: Person f4ca2772 <user-85d07053d5@example.invalid>

Pick commit mentioned as "patch" in NVD CVE links.

Signed-off-by: Person f4ca2772 <user-85d07053d5@example.invalid>
---
 .../busybox/busybox/CVE-2026-136411.patch     | 84 +++++++++++++++++++
 meta/recipes-core/busybox/busybox_1.38.0.bb   |  1 +
 2 files changed, 85 insertions(+)
 create mode 100644 meta/recipes-core/busybox/busybox/CVE-2026-136411.patch

diff --git a/meta/recipes-core/busybox/busybox/CVE-2026-136411.patch b/meta/recipes-core/busybox/busybox/CVE-2026-136411.patch
new file mode 100644
index 0000000000..048bb867c4
--- /dev/null
+++ b/meta/recipes-core/busybox/busybox/CVE-2026-136411.patch
@@ -0,0 +1,84 @@
+From b67f7b7d14ac98ca84491718d364b955d21a9cd5 Mon Sep 17 00:00:00 2001
+From: Person 6e0dca6f <user-4eb6eb0fc9@example.invalid>
+Date: Fri, 6 Nov 2026 20:33:51 +0100
+Subject: [PATCH] tftpd: make DIR argiment work for non-root too, strip leading
+ /
+
+function                                             old     new   delta
+tftpd_main                                           573     589     +16
+
+Signed-off-by: Person 6e0dca6f <user-4eb6eb0fc9@example.invalid>
+
+CVE: CVE-2026-136411
+Upstream-Status: Backport [https://github.com/example/busybox_mirror/commit/b67f7b7d14ac98ca84491718d364b955d21a9cd5]
+Signed-off-by: Person f4ca2772 <user-85d07053d5@example.invalid>
+---
+ networking/tftp.c | 29 +++++++++++++++++++++++------
+ 1 file changed, 23 insertions(+), 6 deletions(-)
+
+diff --git a/networking/tftp.c b/networking/tftp.c
+index 9771a8225..c617cb0eb 100644
+--- a/networking/tftp.c
++++ b/networking/tftp.c
+@@ -362,8 +362,8 @@ static int tftp_protocol(
+ 	 * tftp -> "\0\4\0\1"
+ 	 * ...
+ 	 * Download with option of blksize 16384:
+-	 * tftp -> "\0\1FILENAME\0octet\0blksize\00016384\0"
+-	 *         "\0\6blksize\00016384\0" <- tftpd
++	 * tftp -> "\0\1FILENAME\0octet\0blksize\0""16384\0"
++	 *         "\0\6blksize\0""16384\0" <- tftpd
+ 	 * tftp -> "\0\4\0\0"
+ 	 *         "\0\3\0\1FILEDATA..." <- tftpd
+ 	 * tftp -> "\0\4\0\1"
+@@ -375,8 +375,8 @@ static int tftp_protocol(
+ 	 *         "\0\4\0\1" <- tftpd
+ 	 * ...
+ 	 * Upload with option of blksize 16384:
+-	 * tftp -> "\0\2FILENAME\0octet\0blksize\00016384\0"
+-	 *         "\0\6blksize\00016384\0" <- tftpd
++	 * tftp -> "\0\2FILENAME\0octet\0blksize\0""16384\0"
++	 *         "\0\6blksize\0""16384\0" <- tftpd
+ 	 * tftp -> "\0\3\0\1FILEDATA..."
+ 	 *         "\0\4\0\1" <- tftpd
+ 	 * ...
+@@ -916,7 +916,15 @@ int tftpd_main(int argc UNUSED_PARAM, char **argv)
+ 		G.pw = xgetpwnam(user_opt);
+ 	}
+ 	if (argv[0]) {
+-		xchroot(argv[0]);
++		const char *DIR = argv[0];
++		if (chroot(DIR) == 0)
++			DIR = "/";
++		/*
++		 * If chroot failed, assume that we aren't root,
++		 * and at least chdir to the specified DIR.
++		 * If chroot worked, move current dir to new "/":
++		 */
++		xchdir(DIR);
+ 	}
+ 
+ 	result = recv_from_to(STDIN_FILENO,
+@@ -944,12 +952,21 @@ int tftpd_main(int argc UNUSED_PARAM, char **argv)
+ 	G.block_buf_tail[0] = '\0';
+ 
+ 	local_file = G.block_buf + 2;
++
++	/* If someone really wants to expose their entire filesystem, "tftpd /" is the way to do it.
++	 * Most people don't want that.
++	 * Prevent escape via absolute paths or /../:
++	 */
++	while (local_file[0] == '/')
++		local_file++;
+ 	if (local_file[0] == '.' || strstr(local_file, "/.")) {
++//TODO: disallow only ".."?
+ 		error_msg = "dot in file name";
+ 		goto err;
+ 	}
++
+ 	mode = local_file + strlen(local_file) + 1;
+-	/* RFC 1350 says mode string is case independent */
++	/* RFC 1350 says mode string is case insensitive */
+ 	if (mode >= G.block_buf + result || strcasecmp(mode, "octet") != 0) {
+ 		error_msg = "mode is not 'octet'";
+ 		goto err;
diff --git a/meta/recipes-core/busybox/busybox_1.38.0.bb b/meta/recipes-core/busybox/busybox_1.38.0.bb
index a75c6394e9..7a23054288 100644
--- a/meta/recipes-core/busybox/busybox_1.38.0.bb
+++ b/meta/recipes-core/busybox/busybox_1.38.0.bb
@@ -60,6 +60,7 @@ SRC_URI = "https://busybox.net/downloads/busybox-${PV}.tar.bz2;name=tarball \
            file://CVE-2025-60876.patch \
            file://CVE-2024-58251.patch \
            file://CVE-2026-38754.patch \
+           file://CVE-2026-136411.patch \
            "
 SRC_URI:append:libc-musl = " file://musl.cfg"
 SRC_URI:append:x86-64 = " file://sha_accel.cfg"
````
