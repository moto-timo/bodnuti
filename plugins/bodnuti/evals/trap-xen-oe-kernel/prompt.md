---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [trap]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person 207d817f <user-600ee4795a@example.invalid>
Subject: [meta-virtualization][PATCH 2/2] xen-rpi-u-boot-scr: replace UBOOT_ARCH

Since oe-core:
bdcd04bbc5 "kernel-arch: Drop UBOOT_ARCH", we should just use the
oe.kernel.map_uboot_arch(d) call directly. This was the only
reason for inheriting kernel-arch, so we can drop it.

Signed-off-by: Person 207d817f <user-398c0cb752@example.invalid>
---
 .../recipes-bsp/xen-rpi-u-boot-scr/xen-rpi-u-boot-scr.bb      | 4 ++--
 1 file changed, 2 insertions(+), 2 deletions(-)

diff --git a/dynamic-layers/raspberrypi/recipes-bsp/xen-rpi-u-boot-scr/xen-rpi-u-boot-scr.bb b/dynamic-layers/raspberrypi/recipes-bsp/xen-rpi-u-boot-scr/xen-rpi-u-boot-scr.bb
index e25963ad..7e7b2f6a 100644
--- a/dynamic-layers/raspberrypi/recipes-bsp/xen-rpi-u-boot-scr/xen-rpi-u-boot-scr.bb
+++ b/dynamic-layers/raspberrypi/recipes-bsp/xen-rpi-u-boot-scr/xen-rpi-u-boot-scr.bb
@@ -19,10 +19,10 @@ do_compile() {
         -e 's/@@RPI_DEBUG_XEN_ARGS@@/${RPI_DEBUG_XEN_ARGS}/' \
         "${UNPACKDIR}/boot.cmd.xen.in" > "${WORKDIR}/boot.cmd"
 
-    mkimage -A ${UBOOT_ARCH} -T script -C none -n "Boot script" -d "${WORKDIR}/boot.cmd" boot.scr
+    mkimage -A ${@oe.kernel.map_uboot_arch(d)} -T script -C none -n "Boot script" -d "${WORKDIR}/boot.cmd" boot.scr
 }
 
-inherit kernel-arch deploy nopackages
+inherit deploy nopackages
 
 S = "${UNPACKDIR}"
 
-- 
2.56.1



````
