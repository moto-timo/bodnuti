---
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
tags: [real]
---

This patch was posted to an OpenEmbedded mailing list on 26 November 2026. Review it as a maintainer would. List only real problems, each with the file, the line and what to change. If there are none, say so. You have the mail only, not the layer. Mail addresses and names in it are replaced by placeholders; do not report them.

````
From: Person d3f24025 <user-bf364b04a2@example.invalid>
Subject: [PATCH] linux-firmware: upgrade 20261019 -> 20261124

Upgrade the firmware package to latest release. Add ACP SOF
firmware and topologies, add AIC8800D80N Wi-Fi firmware, add the
2.2.3 (ABI v57) firmware for mm81x driver. Add awinic AW88399
SmartK AMP firmware, add firmware binary for the intel-ipu6 driver 
for IPU8, add Dell ISH firmware for Intel Nova Lake-H. Add firmware
for IW610 WiFi device, add audioreach tplg firmwares for various 
Qualcomm boards, add SOCCP firmware for Qualcomm Nord and Glymur boards, 
Add gpu firmwares for Qualcomm Nord board, and qupv3fw for Qualcomm Eliza board.

Signed-off-by: Person d3f24025 <user-bf364b04a2@example.invalid>
Cc: Person d320be2c <user-7a895009cb@example.invalid>
---
 ...20261019.bb => linux-firmware_20261124.bb} | 208 +++++++++++++++---
 1 file changed, 173 insertions(+), 35 deletions(-)
 rename meta/recipes-kernel/linux-firmware/{linux-firmware_20261019.bb => linux-firmware_20261124.bb} (95%)

diff --git a/meta/recipes-kernel/linux-firmware/linux-firmware_20261019.bb b/meta/recipes-kernel/linux-firmware/linux-firmware_20261124.bb
similarity index 95%
rename from meta/recipes-kernel/linux-firmware/linux-firmware_20261019.bb
rename to meta/recipes-kernel/linux-firmware/linux-firmware_20261124.bb
index 01341a81ff..479c7e4a94 100644
--- a/meta/recipes-kernel/linux-firmware/linux-firmware_20261019.bb
+++ b/meta/recipes-kernel/linux-firmware/linux-firmware_20261124.bb
@@ -31,12 +31,14 @@ LICENSE = "\
     AND LicenseRef-Firmware-amdnpu \
     AND LicenseRef-Firmware-amd-pmf \
     AND LicenseRef-Firmware-amd-sev \
+    AND LicenseRef-Firmware-amdsof \
     AND LicenseRef-Firmware-amd-ucode \
     AND LicenseRef-Firmware-amlogic \
     AND LicenseRef-Firmware-amlogic-vdec \
     AND LicenseRef-Firmware-amphion-vpu \
     AND LicenseRef-Firmware-atheros-firmware \
     AND LicenseRef-Firmware-atmel \
+    AND LicenseRef-Firmware-awinic \
     AND LicenseRef-Firmware-bfa \
     AND LicenseRef-Firmware-bmi260 \
     AND LicenseRef-Firmware-bnx2 \
@@ -164,12 +166,14 @@ LIC_FILES_CHKSUM = "file://LICENSES/LICENCE.Abilis;md5=b5ee3f410780e56711ad48ead
                     file://LICENSES/LICENSE.amdnpu;md5=ea42c0f38f2d42aad08bd50c822460dc \
                     file://LICENSES/LICENSE.amd_pmf;md5=a2589a05ea5b6bd2b7f4f623c7e7a649 \
                     file://LICENSES/LICENSE.amd-sev;md5=e750538791a8be0b7249c579edefb035 \
+                    file://LICENSES/LICENSE.amdsof;md5=010b784dc3bc824bdf66ed74b6bbdb17 \
                     file://LICENSES/LICENSE.amd-ucode;md5=450f217aadc787514165e9568652d700 \
                     file://LICENSES/LICENSE.amlogic;md5=80e4e3f27def8bc4b232009c3a587c07 \
                     file://LICENSES/LICENSE.amlogic_vdec;md5=dc44f59bf64a81643e500ad3f39a468a \
                     file://LICENSES/LICENSE.amphion_vpu;md5=2bcdc00527b2d0542bd92b52aaec2b60 \
                     file://LICENSES/LICENCE.atheros_firmware;md5=30a14c7823beedac9fa39c64fdd01a13 \
                     file://LICENSES/LICENSE.atmel;md5=aa74ac0c60595dee4d4e239107ea77a3 \
+                    file://LICENSES/LICENSE.awinic;md5=2542c2dbd2a6da3cc2cf85387088555f \
                     file://LICENSES/LICENSE.bfa;md5=ff822aa5edf56d0159acfe5a49a5be2c \
                     file://LICENSES/LICENSE.bmi260;md5=0008c039ec4281e382bd0cb41b66866c \
                     file://LICENSES/LICENCE.bnx2;md5=d156fb810e162c4b0065ec8316efcd38 \
@@ -279,7 +283,7 @@ LIC_FILES_CHKSUM = "file://LICENSES/LICENCE.Abilis;md5=b5ee3f410780e56711ad48ead
                     "
 # WHENCE checksum is defined separately to ease overriding it if
 # class-devupstream is selected.
-WHENCE_CHKSUM  = "d1b892bc13f2bc5148ce27c732ec8339"
+WHENCE_CHKSUM  = "5fdc74405b5d6ee112d4f482ccdae43b"
 
 # These are not common licenses, set NO_GENERIC_LICENSE for them
 # so that the license files will be copied from fetched source
@@ -296,12 +300,14 @@ NO_GENERIC_LICENSE[Firmware-amdisp] = "LICENSES/LICENSE.amdisp"
 NO_GENERIC_LICENSE[Firmware-amdnpu] = "LICENSES/LICENSE.amdnpu"
 NO_GENERIC_LICENSE[Firmware-amd-pmf] = "LICENSES/LICENSE.amd_pmf"
 NO_GENERIC_LICENSE[Firmware-amd-sev] = "LICENSES/LICENSE.amd-sev"
+NO_GENERIC_LICENSE[Firmware-amdsof] = "LICENSES/LICENSE.amdsof"
 NO_GENERIC_LICENSE[Firmware-amd-ucode] = "LICENSES/LICENSE.amd-ucode"
 NO_GENERIC_LICENSE[Firmware-amlogic] = "LICENSES/LICENSE.amlogic"
 NO_GENERIC_LICENSE[Firmware-amlogic-vdec] = "LICENSES/LICENSE.amlogic_vdec"
 NO_GENERIC_LICENSE[Firmware-amphion-vpu] = "LICENSES/LICENSE.amphion_vpu"
 NO_GENERIC_LICENSE[Firmware-atheros-firmware] = "LICENSES/LICENCE.atheros_firmware"
 NO_GENERIC_LICENSE[Firmware-atmel] = "LICENSES/LICENSE.atmel"
+NO_GENERIC_LICENSE[Firmware-awinic] = "LICENSES/LICENSE.awinic"
 NO_GENERIC_LICENSE[Firmware-bfa] = "LICENSES/LICENSE.bfa"
 NO_GENERIC_LICENSE[Firmware-bmi260] = "LICENSES/LICENSE.bmi260"
 NO_GENERIC_LICENSE[Firmware-bnx2] = "LICENSES/LICENCE.bnx2"
@@ -419,7 +425,7 @@ SRC_URI:class-devupstream = "git://git.kernel.org/pub/scm/linux/kernel/git/firmw
 # Pin this to the 20220509 release, override this in local.conf
 SRCREV:class-devupstream ?= "b19cbdca78ab2adfd210c91be15a22568e8b8cae"
 
-SRC_URI[sha256sum] = "a10d1ab5baf2c74ba92e8c7cf313e691696c26bddfa6df00db795e48d70cd4ca"
+SRC_URI[sha256sum] = "2cabe10253005f55bf4bbc0ebb25c0964d66f7c4121ffa47853ff1e70ea1aa54"
 
 inherit allarch
 
@@ -615,7 +621,7 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-ath12k-qcc2072 \
              ${PN}-ath12k-qcn9274 \
              ${PN}-ath12k-wcn7850 \
-             ${PN}-aic-aic8800d80 ${PN}-aic-license \
+             ${PN}-aic-aic8800d80 ${PN}-aic-aic8800d80n ${PN}-aic-license \
              ${PN}-qca \
              ${PN}-qca-qca2066 \
              ${PN}-qca-qca61x4-serial \
@@ -679,6 +685,7 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-nxp9098-pcie \
              ${PN}-nxp9098-sdio \
              ${PN}-nxpiw416-sdio \
+             ${PN}-nxpiw610-sdio \
              ${PN}-nxpiw612-sdio \
              ${PN}-nxpiw61x-sdio \
              ${PN}-nxp-sr1xx \
@@ -698,18 +705,22 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-qcom-adreno-a612 ${PN}-qcom-adreno-a623 ${PN}-qcom-adreno-a630 ${PN}-qcom-adreno-a640 \
              ${PN}-qcom-adreno-a650 ${PN}-qcom-adreno-a660 ${PN}-qcom-adreno-a663 \
              ${PN}-qcom-adreno-a702 ${PN}-qcom-adreno-a730 ${PN}-qcom-adreno-a740 \
-             ${PN}-qcom-adreno-g705 ${PN}-qcom-adreno-g709 ${PN}-qcom-adreno-g715  ${PN}-qcom-adreno-g717 \
+             ${PN}-qcom-adreno-g705 ${PN}-qcom-adreno-g709 ${PN}-qcom-adreno-g713 \
+             ${PN}-qcom-adreno-g715 ${PN}-qcom-adreno-g716 ${PN}-qcom-adreno-g717 \
              ${PN}-qcom-adreno-g800 ${PN}-qcom-adreno-g801 ${PN}-qcom-adreno-g802 \
              ${PN}-qcom-adreno-g811 ${PN}-qcom-adreno-g814 \
              ${PN}-qcom-apq8016-modem ${PN}-qcom-apq8016-wifi \
              ${PN}-qcom-apq8096-adreno ${PN}-qcom-apq8096-audio ${PN}-qcom-apq8096-modem \
-             ${PN}-qcom-eliza-adreno ${PN}-qcom-eliza-audio ${PN}-qcom-eliza-compute \
-             ${PN}-qcom-glymur-adreno ${PN}-qcom-glymur-audio ${PN}-qcom-glymur-compute \
+             ${PN}-qcom-eliza-adreno ${PN}-qcom-eliza-audio ${PN}-qcom-eliza-compute ${PN}-qcom-eliza-qupv3fw \
+             ${PN}-qcom-glymur-adreno ${PN}-qcom-glymur-asus-zenbook-a14-audio-tplg \
+             ${PN}-qcom-glymur-asus-zenbook-a16-audio-tplg  ${PN}-qcom-glymur-audio ${PN}-qcom-glymur-compute \
+             ${PN}-qcom-glymur-lenovo-t14s-g7-audio-tplg  ${PN}-qcom-glymur-soccp \
              ${PN}-qcom-hawi-adreno ${PN}-qcom-hawi-audio ${PN}-qcom-hawi-compute \
              ${PN}-qcom-kaanapali-adreno ${PN}-qcom-kaanapali-audio \
              ${PN}-qcom-kaanapali-compute ${PN}-qcom-kaanapali-soccp \
              ${PN}-qcom-maili-adreno ${PN}-qcom-maili-audio \
-             ${PN}-qcom-nord-audio ${PN}-qcom-nord-compute ${PN}-qcom-nord-qupv3fw \
+             ${PN}-qcom-nord-adreno ${PN}-qcom-nord-audio ${PN}-qcom-nord-compute \
+             ${PN}-qcom-nord-qupv3fw ${PN}-qcom-nord-soccp \
              ${PN}-qcom-qcm2290-adreno ${PN}-qcom-qcm2290-audio ${PN}-qcom-qcm2290-modem \
              ${PN}-qcom-qcm6490-adreno ${PN}-qcom-qcm6490-audio ${PN}-qcom-qcm6490-compute \
              ${PN}-qcom-qcm6490-ipa ${PN}-qcom-qcm6490-wifi ${PN}-qcom-qcm6490-qupv3fw \
@@ -754,6 +765,7 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-qcom-x1e80100-dell-xps13-9345-compute \
              ${PN}-qcom-x1e80100-hp-omnibook-x14-audio-tplg \
              ${PN}-qcom-x1e80100-adreno ${PN}-qcom-x1e80100-audio ${PN}-qcom-x1e80100-compute \
+             ${PN}-qcom-x1e80100-lenovo-ideacenter-mini-audio-tplg \
              ${PN}-qcom-x1e80100-lenovo-t14s-g6-adreno ${PN}-qcom-x1e80100-lenovo-t14s-g6-audio \
              ${PN}-qcom-x1e80100-lenovo-t14s-g6-compute ${PN}-qcom-x1e80100-lenovo-t14s-g6-vpu \
              ${PN}-qcom-x1e80100-lenovo-yoga-slim7x-adreno ${PN}-qcom-x1e80100-lenovo-yoga-slim7x-audio \
@@ -770,7 +782,7 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-xc5000-license ${PN}-xc5000 \
              ${PN}-xc5000c-license ${PN}-xc5000c \
              ${PN}-typhoon-license ${PN}-typhoon \
-             ${PN}-intel-license ${PN}-ish-lnlm ${PN}-ish-nvlh ${PN}-ish-nvls \
+             ${PN}-intel-license ${PN}-ish-lnlm ${PN}-ish-nvlh ${PN}-ish-nvlh-39ceeaf8 ${PN}-ish-nvls \
              ${PN}-dell-license ${PN}-ish-lnlm-39ceeaf8 ${PN}-ish-ptl-39ceeaf8 \
              ${PN}-hp-license ${PN}-ish-lnlm-12128606 ${PN}-ish-ptl-12128606 \
              ${PN}-lenovo-license ${PN}-ish-lnlm-53c4ffad ${PN}-ish-ptl-53c4ffad \
@@ -780,11 +792,13 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-agere-license ${PN}-orinoco \
              ${PN}-airoha-license ${PN}-an8811hb ${PN}-en8811h ${PN}-airoha-npu \
              ${PN}-amd-sev-license ${PN}-ccp \
+             ${PN}-amdsof-license ${PN}-amdsof-acp7f ${PN}-amdsof-acp7f-tplg \
              ${PN}-amdnpu-license ${PN}-amdxdna \
              ${PN}-amd-pmf-license ${PN}-amd-pmf \
              ${PN}-amd-ucode-license ${PN}-microcode-amd \
              ${PN}-amlogic-license ${PN}-amlogic \
              ${PN}-abilis-license ${PN}-as102 \
+             ${PN}-awinic-license ${PN}-awinic-aw88399 \
              ${PN}-starfire \
              ${PN}-atmel-license ${PN}-wilc1000 ${PN}-wilc3000 \
              ${PN}-atusb \
@@ -816,9 +830,9 @@ PACKAGES =+ "${PN}-amphion-vpu-license ${PN}-amphion-vpu \
              ${PN}-inside-secure-license ${PN}-inside-secure \
              ${PN}-intcsst2-license ${PN}-snd-soc-catpt \
              ${PN}-fw-sst-0f28-license ${PN}-snd-intel-sst-core \
-             ${PN}-ivsc-license ${PN}-atomisp ${PN}-intel-ipu6-isys ${PN}-mei-vsc-hw \
+             ${PN}-ivsc-license ${PN}-atomisp ${PN}-mei-vsc-hw \
              ${PN}-ipu3-firmware-license ${PN}-ipu3-imgu \
-             ${PN}-intel-ipu7-isys \
+             ${PN}-intel-ipu6-isys ${PN}-intel-ipu7-isys ${PN}-intel-ipu8-isys \
              ${PN}-intel-vpu-license ${PN}-intel-vpu \
              ${PN}-isci \
              ${PN}-ixp4xx-license ${PN}-ixp4xx-npe \
@@ -948,13 +962,14 @@ LICENSE:${PN}-ath12k-wcn7850 = "LicenseRef-Firmware-qualcommAthos-ath10k"
 
 LICENSE:${PN}-aic-license = "LicenseRef-Firmware-aic"
 LICENSE:${PN}-aic-aic8800d80 = "LicenseRef-Firmware-aic"
+LICENSE:${PN}-aic-aic8800d80n = "LicenseRef-Firmware-aic"
 
 LICENSE:${PN}-qca-qca2066 = "LicenseRef-Firmware-qualcommAthos-ath10k"
 LICENSE:${PN}-qca-qca61x4-serial = "LicenseRef-Firmware-qualcommAthos-ath10k"
 LICENSE:${PN}-qca-qca61x4-usb = "LicenseRef-Firmware-qualcommAthos-ath10k"
 LICENSE:${PN}-qca-qca6390 = "LicenseRef-Firmware-qualcommAthos-ath10k"
 LICENSE:${PN}-qca-qca6698 = "LicenseRef-Firmware-qualcommAthos-ath10k"
-LICENSE:${PN}-qca-qcc2072 = "LicenseRef-Firmware-qualcommAthos-ath10k"
+LICENSE:${PN}-qca-qcc2072 = "LicenseRef-Firmware-qualcommAthos-ath10k AND LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qca-wcn3950 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qca-wcn3988 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qca-wcn399x = "LicenseRef-Firmware-qualcommAthos-ath10k"
@@ -1001,6 +1016,7 @@ ALLOW_EMPTY:${PN}-ath12k = "1"
 
 FILES:${PN}-aic-license = "${firmwaredir}/LICENSE.aic"
 FILES:${PN}-aic-aic8800d80 = "${firmwaredir}/aic/aic8800d80/*"
+FILES:${PN}-aic-aic8800d80n = "${firmwaredir}/aic/aic8800d80n/*"
 
 FILES:${PN}-qca-qca2066 = " \
   ${firmwaredir}/qca/hpbtfw21.tlv* \
@@ -1066,6 +1082,11 @@ FILES:${PN}-qca-qcc2072 = " \
   ${firmwaredir}/qca/ornbtfw11.tlv* \
   ${firmwaredir}/qca/ornnv10.bin* \
   ${firmwaredir}/qca/ornnv11.bin* \
+  ${firmwaredir}/qca/ornnv11.b18* \
+  ${firmwaredir}/qca/QCC2072/nvm_usb_00200101*.bin* \
+  ${firmwaredir}/qca/QCC2072/perinvm_usb_00220100*.bin* \
+  ${firmwaredir}/qca/QCC2072/peripatch_usb_00220100.bin* \
+  ${firmwaredir}/qca/QCC2072/rampatch_usb_00200101*.bin* \
 "
 FILES:${PN}-qca-wcn3950 = " \
   ${firmwaredir}/qca/cmbtfw12.tlv* \
@@ -1140,6 +1161,7 @@ RDEPENDS:${PN}-ath12k-qcn9274 += "${PN}-ath10k-license"
 RDEPENDS:${PN}-ath12k-wcn7850 += "${PN}-ath10k-license"
 
 RDEPENDS:${PN}-aic-aic8800d80 += "${PN}-aic-license"
+RDEPENDS:${PN}-aic-aic8800d80n += "${PN}-aic-license"
 
 RDEPENDS:${PN}-qca += "${PN}-ath10k-license ${PN}-qcom-license"
 RDEPENDS:${PN}-qca-qca2066 += "${PN}-ath10k-license"
@@ -1147,7 +1169,7 @@ RDEPENDS:${PN}-qca-qca61x4-serial += "${PN}-ath10k-license"
 RDEPENDS:${PN}-qca-qca61x4-usb += "${PN}-ath10k-license"
 RDEPENDS:${PN}-qca-qca6390 += "${PN}-ath10k-license"
 RDEPENDS:${PN}-qca-qca6698 += "${PN}-ath10k-license"
-RDEPENDS:${PN}-qca-qcc2072 += "${PN}-ath10k-license"
+RDEPENDS:${PN}-qca-qcc2072 += "${PN}-ath10k-license ${PN}-qcom-license"
 RDEPENDS:${PN}-qca-wcn3950 += "${PN}-qcom-license"
 RDEPENDS:${PN}-qca-wcn3988 += "${PN}-qcom-license"
 RDEPENDS:${PN}-qca-wcn399x += "${PN}-ath10k-license"
@@ -1551,6 +1573,7 @@ LICENSE:${PN}-nxp9098-common = "LicenseRef-Firmware-nxp"
 LICENSE:${PN}-nxp9098-pcie = "LicenseRef-Firmware-nxp"
 LICENSE:${PN}-nxp9098-sdio = "LicenseRef-Firmware-nxp"
 LICENSE:${PN}-nxpiw416-sdio = "LicenseRef-Firmware-nxp"
+LICENSE:${PN}-nxpiw610-sdio = "LicenseRef-Firmware-nxp"
 LICENSE:${PN}-nxpiw612-sdio = "LicenseRef-Firmware-nxp"
 LICENSE:${PN}-nxpiw61x-sdio = "LicenseRef-Firmware-nxp"
 LICENSE:${PN}-nxp-sr1xx = "LicenseRef-Firmware-nxp"
@@ -1567,6 +1590,7 @@ FILES:${PN}-nxp9098-common = "${firmwaredir}/nxp/uartuart9098_bt_v1.bin*"
 ALLOW_EMPTY:${PN}-nxp9098-pcie = "1"
 ALLOW_EMPTY:${PN}-nxp9098-sdio = "1"
 FILES:${PN}-nxpiw416-sdio = "${firmwaredir}/nxp/*iw416*"
+FILES:${PN}-nxpiw610-sdio = "${firmwaredir}/nxp/sd_iw610.bin.se*"
 FILES:${PN}-nxpiw612-sdio = "${firmwaredir}/nxp/uartspi_n61x_v1.bin.se*"
 FILES:${PN}-nxpiw61x-sdio = "${firmwaredir}/nxp/sd_w61x_v1.bin.se*"
 FILES:${PN}-nxp-sr1xx = "${firmwaredir}/nxp/sr150_fw.bin*"
@@ -1580,6 +1604,7 @@ RDEPENDS:${PN}-nxp9098-common += "${PN}-nxp-license"
 RDEPENDS:${PN}-nxp9098-pcie += "${PN}-nxp9098-common"
 RDEPENDS:${PN}-nxp9098-sdio += "${PN}-nxp9098-common"
 RDEPENDS:${PN}-nxpiw416-sdio += "${PN}-nxp-license"
+RDEPENDS:${PN}-nxpiw610-sdio += "${PN}-nxp-license"
 RDEPENDS:${PN}-nxpiw612-sdio += "${PN}-nxp-license"
 RDEPENDS:${PN}-nxpiw61x-sdio += "${PN}-nxp-license"
 RDEPENDS:${PN}-nxp-sr1xx += "${PN}-nxp-license"
@@ -2429,7 +2454,9 @@ LICENSE:${PN}-qcom-adreno-a730 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-a740 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-g705 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-g709 = "LicenseRef-Firmware-qcom"
+LICENSE:${PN}-qcom-adreno-g713 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-g715 = "LicenseRef-Firmware-qcom"
+LICENSE:${PN}-qcom-adreno-g716 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-g717 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-g800 = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-adreno-g801 = "LicenseRef-Firmware-qcom"
@@ -2442,23 +2469,30 @@ LICENSE:${PN}-qcom-apq8096-audio = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-apq8096-adreno = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-apq8096-modem = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-eliza-adreno = "LicenseRef-Firmware-qcom"
-LICENSE:${PN}-qcom-eliza-audio = "LicenseRef-Firmware-qcom-2"
+LICENSE:${PN}-qcom-eliza-audio = "LicenseRef-Firmware-qcom-2 AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-eliza-compute = "LicenseRef-Firmware-qcom-2"
+LICENSE:${PN}-qcom-eliza-qupv3fw = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-glymur-adreno = "LicenseRef-Firmware-qcom"
+LICENSE:${PN}-qcom-glymur-asus-zenbook-a14-audio-tplg = "LicenseRef-Firmware-linaro"
+LICENSE:${PN}-qcom-glymur-asus-zenbook-a16-audio-tplg = "LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-glymur-audio = "LicenseRef-Firmware-qcom-2 AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-glymur-compute = "LicenseRef-Firmware-qcom-2"
+LICENSE:${PN}-qcom-glymur-lenovo-t14s-g7-audio-tplg = "LicenseRef-Firmware-linaro"
+LICENSE:${PN}-qcom-glymur-soccp = "LicenseRef-Firmware-qcom-2"
 LICENSE:${PN}-qcom-hawi-adreno = "LicenseRef-Firmware-qcom"
-LICENSE:${PN}-qcom-hawi-audio = "LicenseRef-Firmware-qcom-2"
+LICENSE:${PN}-qcom-hawi-audio = "LicenseRef-Firmware-qcom-2 AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-hawi-compute = "LicenseRef-Firmware-qcom-2"
 LICENSE:${PN}-qcom-kaanapali-adreno = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-kaanapali-audio = "LicenseRef-Firmware-qcom-2 AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-kaanapali-compute = "LicenseRef-Firmware-qcom-2"
 LICENSE:${PN}-qcom-kaanapali-soccp = "LicenseRef-Firmware-qcom-2"
 LICENSE:${PN}-qcom-maili-adreno = "LicenseRef-Firmware-qcom"
-LICENSE:${PN}-qcom-maili-audio = "LicenseRef-Firmware-qcom-2"
-LICENSE:${PN}-qcom-nord-audio = "LicenseRef-Firmware-qcom-2"
+LICENSE:${PN}-qcom-maili-audio = "LicenseRef-Firmware-qcom-2 AND LicenseRef-Firmware-linaro"
+LICENSE:${PN}-qcom-nord-adreno = "LicenseRef-Firmware-qcom"
+LICENSE:${PN}-qcom-nord-audio = "LicenseRef-Firmware-qcom-2 AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-nord-compute = "LicenseRef-Firmware-qcom-2"
 LICENSE:${PN}-qcom-nord-qupv3fw = "LicenseRef-Firmware-qcom"
+LICENSE:${PN}-qcom-nord-soccp = "LicenseRef-Firmware-qcom-2"
 LICENSE:${PN}-qcom-qcm2290-adreno = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-qcm2290-audio = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-qcm2290-modem = "LicenseRef-Firmware-qcom"
@@ -2496,7 +2530,7 @@ LICENSE:${PN}-qcom-sc8280xp-lenovo-x13s-adreno = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-sc8280xp-lenovo-x13s-compute = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-sc8280xp-lenovo-x13s-sensors = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-sc8280xp-lenovo-x13s-vpu = "LicenseRef-Firmware-qcom"
-LICENSE:${PN}-qcom-sc8280xp-radxa-dragon-q8b-audio = "LicenseRef-Firmware-qcom"
+LICENSE:${PN}-qcom-sc8280xp-radxa-dragon-q8b-audio = "LicenseRef-Firmware-qcom AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-sdm845-audio = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-sdm845-adreno = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-sdm845-compute = "LicenseRef-Firmware-qcom"
@@ -2540,6 +2574,7 @@ LICENSE:${PN}-qcom-x1e80100-dell-xps13-9345-adreno = "LicenseRef-Firmware-dell"
 LICENSE:${PN}-qcom-x1e80100-dell-xps13-9345-audio = "LicenseRef-Firmware-dell AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-x1e80100-dell-xps13-9345-compute = "LicenseRef-Firmware-dell"
 LICENSE:${PN}-qcom-x1e80100-hp-omnibook-x14-audio-tplg = "LicenseRef-Firmware-linaro"
+LICENSE:${PN}-qcom-x1e80100-lenovo-ideacenter-mini-audio-tplg = "LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-x1e80100-lenovo-t14s-g6-adreno = "LicenseRef-Firmware-qcom"
 LICENSE:${PN}-qcom-x1e80100-lenovo-t14s-g6-audio = "LicenseRef-Firmware-qcom AND LicenseRef-Firmware-linaro"
 LICENSE:${PN}-qcom-x1e80100-lenovo-t14s-g6-compute = "LicenseRef-Firmware-qcom"
@@ -2584,7 +2619,9 @@ FILES:${PN}-qcom-adreno-a730 = "${firmwaredir}/qcom/a730_sqe*.* ${firmwaredir}/q
 FILES:${PN}-qcom-adreno-a740 = "${firmwaredir}/qcom/a740_sqe*.* ${firmwaredir}/qcom/gmu_gen70200.*"
 FILES:${PN}-qcom-adreno-g705 = "${firmwaredir}/qcom/gen70500_*.*"
 FILES:${PN}-qcom-adreno-g709 = "${firmwaredir}/qcom/gen70900_*.* ${firmwaredir}/qcom/gmu_gen70900.*"
+FILES:${PN}-qcom-adreno-g713 = "${firmwaredir}/qcom/gen71300_*.*"
 FILES:${PN}-qcom-adreno-g715 = "${firmwaredir}/qcom/gen71500_*.*"
+FILES:${PN}-qcom-adreno-g716 = "${firmwaredir}/qcom/gen71600_*.*"
 FILES:${PN}-qcom-adreno-g717 = "${firmwaredir}/qcom/gen71700_*.*"
 FILES:${PN}-qcom-adreno-g800 = "${firmwaredir}/qcom/gen80000_*.*"
 FILES:${PN}-qcom-adreno-g801 = "${firmwaredir}/qcom/gen80100_*.*"
@@ -2597,16 +2634,36 @@ FILES:${PN}-qcom-apq8096-adreno = "${firmwaredir}/qcom/apq8096/a530_zap.mbn* ${f
 FILES:${PN}-qcom-apq8096-audio = "${firmwaredir}/qcom/apq8096/adsp*.*"
 FILES:${PN}-qcom-apq8096-modem = "${firmwaredir}/qcom/apq8096/mba.mbn* ${firmwaredir}/qcom/apq8096/modem*.* ${firmwaredir}/qcom/apq8096/wlanmdsp.mbn*"
 FILES:${PN}-qcom-eliza-adreno = "${firmwaredir}/qcom/eliza/gen71700_zap.mbn*"
-FILES:${PN}-qcom-eliza-audio = "${firmwaredir}/qcom/eliza/adsp*.*"
+FILES:${PN}-qcom-eliza-audio = " \
+    ${firmwaredir}/qcom/eliza/adsp*.* \
+    ${firmwaredir}/qcom/eliza/eliza-cqs-evk-tplg.bin* \
+"
 FILES:${PN}-qcom-eliza-compute = "${firmwaredir}/qcom/eliza/cdsp*.*"
+FILES:${PN}-qcom-eliza-qupv3fw = "${firmwaredir}/qcom/eliza/qupv3fw.elf*"
 FILES:${PN}-qcom-glymur-adreno = "${firmwaredir}/qcom/glymur/gen80100_zap.mbn*"
+FILES:${PN}-qcom-glymur-asus-zenbook-a14-audio-tplg = " \
+    ${firmwaredir}/qcom/glymur/GLYMUR-ASUS-Zenbook-A14-UX3407NA-tplg.bin* \
+    ${firmwaredir}/qcom/glymur/ASUSTeK/UX3407NA/GLYMUR-ASUS-Zenbook-A14-UX3407NA-tplg.bin* \
+"
+FILES:${PN}-qcom-glymur-asus-zenbook-a16-audio-tplg = " \
+    ${firmwaredir}/qcom/glymur/GLYMUR-ASUS-Zenbook-A16-UX3607OA-tplg.bin* \
+    ${firmwaredir}/qcom/glymur/ASUSTeK/UX3607OA/GLYMUR-ASUS-Zenbook-A16-UX3607OA-tplg.bin* \
+"
 FILES:${PN}-qcom-glymur-audio = " \
     ${firmwaredir}/qcom/glymur/adsp*.* \
     ${firmwaredir}/qcom/glymur/GLYMUR-CRD-tplg.bin* \
 "
 FILES:${PN}-qcom-glymur-compute = "${firmwaredir}/qcom/glymur/cdsp*.*"
+FILES:${PN}-qcom-glymur-lenovo-t14s-g7-audio-tplg = " \
+    ${firmwaredir}/qcom/glymur/Lenovo-ThinkPad-T14s-Gen-7-tplg.bin* \
+    ${firmwaredir}/qcom/glymur/LENOVO/22A8/Lenovo-ThinkPad-T14s-Gen-7-tplg.bin* \
+"
+FILES:${PN}-qcom-glymur-soccp = "${firmwaredir}/qcom/glymur/soccp*.*"
 FILES:${PN}-qcom-hawi-adreno = "${firmwaredir}/qcom/hawi/gen81100_zap.mbn*"
-FILES:${PN}-qcom-hawi-audio = "${firmwaredir}/qcom/hawi/adsp*.*"
+FILES:${PN}-qcom-hawi-audio = " \
+    ${firmwaredir}/qcom/hawi/adsp*.* \
+    ${firmwaredir}/qcom/hawi/HAWI-MTP-tplg.bin* \
+"
 FILES:${PN}-qcom-hawi-compute = "${firmwaredir}/qcom/hawi/cdsp*.*"
 FILES:${PN}-qcom-kaanapali-adreno = "${firmwaredir}/qcom/kaanapali/gen80200_zap.mbn*"
 FILES:${PN}-qcom-kaanapali-audio = " \
@@ -2616,10 +2673,19 @@ FILES:${PN}-qcom-kaanapali-audio = " \
 FILES:${PN}-qcom-kaanapali-compute = "${firmwaredir}/qcom/kaanapali/cdsp*.*"
 FILES:${PN}-qcom-kaanapali-soccp = "${firmwaredir}/qcom/kaanapali/soccp*.*"
 FILES:${PN}-qcom-maili-adreno = "${firmwaredir}/qcom/maili/gen81400_zap.mbn*"
-FILES:${PN}-qcom-maili-audio = "${firmwaredir}/qcom/maili/adsp*.*"
-FILES:${PN}-qcom-nord-audio = "${firmwaredir}/qcom/nord/adsp*.*"
+FILES:${PN}-qcom-maili-audio = " \
+    ${firmwaredir}/qcom/maili/adsp*.* \
+    ${firmwaredir}/qcom/maili/MAILI-MTP-tplg.bin* \
+"
+FILES:${PN}-qcom-nord-adreno = "${firmwaredir}/qcom/nord/gen71300_zap.mbn* ${firmwaredir}/qcom/nord/gen71600_zap.mbn*"
+FILES:${PN}-qcom-nord-audio = " \
+    ${firmwaredir}/qcom/nord/adsp*.* \
+    ${firmwaredir}/qcom/nord/NORD-RIDE-tplg.bin* \
+    ${firmwaredir}/qcom/nord/NORD-RRD-tplg.bin* \
+"
 FILES:${PN}-qcom-nord-compute = "${firmwaredir}/qcom/nord/cdsp*.*"
 FILES:${PN}-qcom-nord-qupv3fw = "${firmwaredir}/qcom/nord/qupv3fw.elf*"
+FILES:${PN}-qcom-nord-soccp = "${firmwaredir}/qcom/nord/soccp*.*"
 FILES:${PN}-qcom-qcm2290-adreno = "${firmwaredir}/qcom/qcm2290/a702_zap.mbn*"
 FILES:${PN}-qcom-qcm2290-audio = "${firmwaredir}/qcom/qcm2290/adsp*.*"
 FILES:${PN}-qcom-qcm2290-modem = "${firmwaredir}/qcom/qcm2290/modem*.*"
@@ -2682,7 +2748,10 @@ FILES:${PN}-qcom-sc8280xp-lenovo-x13s-adreno = "${firmwaredir}/qcom/sc8280xp/LEN
 FILES:${PN}-qcom-sc8280xp-lenovo-x13s-compute = "${firmwaredir}/qcom/sc8280xp/LENOVO/21BX/*cdsp*.*"
 FILES:${PN}-qcom-sc8280xp-lenovo-x13s-sensors = "${firmwaredir}/qcom/sc8280xp/LENOVO/21BX/*slpi*.*"
 FILES:${PN}-qcom-sc8280xp-lenovo-x13s-vpu = "${firmwaredir}/qcom/sc8280xp/LENOVO/21BX/qcvss8280.mbn*"
-FILES:${PN}-qcom-sc8280xp-radxa-dragon-q8b-audio = "${firmwaredir}/qcom/sc8280xp/radxa/dragon-q8b/qcadsp8280.mbn*"
+FILES:${PN}-qcom-sc8280xp-radxa-dragon-q8b-audio = " \
+    ${firmwaredir}/qcom/sc8280xp/radxa/dragon-q8b/qcadsp8280.mbn* \
+    ${firmwaredir}/qcom/sc8280xp/radxa/dragon-q8b/SC8280XP-Radxa-Dragon-Q8B-tplg.bin* \
+"
 FILES:${PN}-qcom-sdm845-adreno = "${firmwaredir}/qcom/sdm845/a630*.*"
 FILES:${PN}-qcom-sdm845-audio = "${firmwaredir}/qcom/sdm845/adsp*.*"
 FILES:${PN}-qcom-sdm845-compute = "${firmwaredir}/qcom/sdm845/cdsp*.*"
@@ -2696,7 +2765,11 @@ FILES:${PN}-qcom-sdx35-foxconn-firehose = "${firmwaredir}/qcom/sdx35/foxconn/xbl
 FILES:${PN}-qcom-sdx61-foxconn-firehose = "${firmwaredir}/qcom/sdx61/foxconn/prog_firehose_lite.elf*"
 FILES:${PN}-qcom-shikra-adreno = "${nonarch_base_libdir}/firmware/qcom/shikra/a704_zap.mbn*"
 FILES:${PN}-qcom-shikra-compute = "${firmwaredir}/qcom/shikra/cdsp*.*"
-FILES:${PN}-qcom-shikra-audio = "${nonarch_base_libdir}/firmware/qcom/shikra/lpaicp*.*"
+FILES:${PN}-qcom-shikra-audio = " \
+    ${nonarch_base_libdir}/firmware/qcom/shikra/lpaicp*.* \
+    ${nonarch_base_libdir}/firmware/qcom/shikra/shikra-cqs-evk-tplg.bin* \
+    ${nonarch_base_libdir}/firmware/qcom/shikra/shikra-iqs-evk-tplg.bin* \
+"
 FILES:${PN}-qcom-shikra-modem = "${nonarch_base_libdir}/firmware/qcom/shikra/cqs/qdsp6sw.mbn*"
 FILES:${PN}-qcom-shikra-qupv3fw = "${firmwaredir}/qcom/shikra/qupv3fw.elf*"
 FILES:${PN}-qcom-shikra-wifi = "${firmwaredir}/qcom/shikra/wlanmdsp.mbn* ${firmwaredir}/ath10k/WCN3990/hw1.0/shikra/*"
@@ -2722,9 +2795,12 @@ FILES:${PN}-qcom-x1e80100-adreno = "${firmwaredir}/qcom/x1e80100/gen70500_zap.mb
 FILES:${PN}-qcom-x1e80100-audio = " \
     ${firmwaredir}/qcom/x1e80100/adsp*.* \
     ${firmwaredir}/qcom/x1e80100/battmgr.jsn \
+    ${firmwaredir}/qcom/x1e80100/X1E80100-ACER-SWIFT-14-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1E001DE-DEVKIT-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1E80100-CRD-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1E80100-EVK-tplg.bin* \
+    ${firmwaredir}/qcom/x1e80100/X1E80100-MEDION-SPRCHRGD-14-S1-tplg.bin* \
+    ${firmwaredir}/qcom/x1e80100/X1E80100-Microsoft-Surface-Pro-11-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1P42100-Microsoft-Surface-Pro-12in-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1E80100-Romulus-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1E80100-TUXEDO-Elite-14-tplg.bin* \
@@ -2762,8 +2838,17 @@ FILES:${PN}-qcom-x1e80100-hp-omnibook-x14-audio-tplg = " \
     ${firmwaredir}/qcom/x1e80100/hp/omnibook-x14/X1E80100-HP-OMNIBOOK-X14-tplg.bin* \
     ${firmwaredir}/qcom/x1e80100/X1E80100-HP-OMNIBOOK-X14-tplg.bin* \
 "
+FILES:${PN}-qcom-x1e80100-lenovo-ideacenter-mini-audio-tplg = " \
+    ${firmwaredir}/qcom/x1e80100/X1E80100-LENOVO-IdeaCentre-Mini-tplg.bin* \
+    ${firmwaredir}/qcom/x1e80100/LENOVO/91B6/X1E80100-LENOVO-IdeaCentre-Mini-tplg.bin* \
+"
 FILES:${PN}-qcom-x1e80100-lenovo-t14s-g6-adreno = "${firmwaredir}/qcom/x1e80100/LENOVO/21N1/qcdxkmsuc8380.mbn*"
-FILES:${PN}-qcom-x1e80100-lenovo-t14s-g6-audio = "${firmwaredir}/qcom/x1e80100/LENOVO/21N1/*adsp*.* ${firmwaredir}/qcom/x1e80100/LENOVO/21N1/battmgr.jsn* ${firmwaredir}/qcom/x1e80100/LENOVO/21N1/X1E80100-LENOVO-Thinkpad-T14s-tplg.bin* ${firmwaredir}/qcom/x1e80100/X1E80100-LENOVO-Thinkpad-T14s-tplg.bin*"
+FILES:${PN}-qcom-x1e80100-lenovo-t14s-g6-audio = " \
+    ${firmwaredir}/qcom/x1e80100/LENOVO/21N1/*adsp*.* \
+    ${firmwaredir}/qcom/x1e80100/LENOVO/21N1/battmgr.jsn* \
+    ${firmwaredir}/qcom/x1e80100/LENOVO/21N1/X1E80100-LENOVO-Thinkpad-T14s-tplg.bin* \
+    ${firmwaredir}/qcom/x1e80100/X1E80100-LENOVO-Thinkpad-T14s-tplg.bin* \
+"
 FILES:${PN}-qcom-x1e80100-lenovo-t14s-g6-compute = "${firmwaredir}/qcom/x1e80100/LENOVO/21N1/*cdsp*.*"
 FILES:${PN}-qcom-x1e80100-lenovo-t14s-g6-vpu = "${firmwaredir}/qcom/x1e80100/LENOVO/21N1/qcvss8380.mbn*"
 FILES:${PN}-qcom-x1e80100-lenovo-yoga-slim7x-adreno = "${firmwaredir}/qcom/x1e80100/LENOVO/83ED/qcdxkmsuc8380.mbn*"
@@ -2798,7 +2883,9 @@ RDEPENDS:${PN}-qcom-adreno-a730 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-a740 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-g705 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-g709 = "${PN}-qcom-license"
+RDEPENDS:${PN}-qcom-adreno-g713 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-g715 = "${PN}-qcom-license"
+RDEPENDS:${PN}-qcom-adreno-g716 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-g717 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-g800 = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-adreno-g801 = "${PN}-qcom-license"
@@ -2811,23 +2898,30 @@ RDEPENDS:${PN}-qcom-apq8096-adreno = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-apq8096-audio = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-apq8096-modem = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-eliza-adreno = "${PN}-qcom-license"
-RDEPENDS:${PN}-qcom-eliza-audio = "${PN}-qcom-2-license"
+RDEPENDS:${PN}-qcom-eliza-audio = "${PN}-qcom-2-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-eliza-compute = "${PN}-qcom-2-license"
+RDEPENDS:${PN}-qcom-eliza-qupv3fw = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-glymur-adreno = "${PN}-qcom-license"
+RDEPENDS:${PN}-qcom-glymur-asus-zenbook-a14-audio-tplg = "${PN}-linaro-license"
+RDEPENDS:${PN}-qcom-glymur-asus-zenbook-a16-audio-tplg = "${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-glymur-audio = "${PN}-qcom-2-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-glymur-compute = "${PN}-qcom-2-license"
+RDEPENDS:${PN}-qcom-glymur-lenovo-t14s-g7-audio-tplg = "${PN}-linaro-license"
+RDEPENDS:${PN}-qcom-glymur-soccp = "${PN}-qcom-2-license"
 RDEPENDS:${PN}-qcom-hawi-adreno = "${PN}-qcom-license"
-RDEPENDS:${PN}-qcom-hawi-audio = "${PN}-qcom-2-license"
+RDEPENDS:${PN}-qcom-hawi-audio = "${PN}-qcom-2-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-hawi-compute = "${PN}-qcom-2-license"
 RDEPENDS:${PN}-qcom-kaanapali-adreno = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-kaanapali-audio = "${PN}-qcom-2-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-kaanapali-compute = "${PN}-qcom-2-license"
 RDEPENDS:${PN}-qcom-kaanapali-soccp = "${PN}-qcom-2-license"
 RDEPENDS:${PN}-qcom-maili-adreno = "${PN}-qcom-license"
-RDEPENDS:${PN}-qcom-maili-audio = "${PN}-qcom-2-license"
-RDEPENDS:${PN}-qcom-nord-audio = "${PN}-qcom-2-license"
+RDEPENDS:${PN}-qcom-maili-audio = "${PN}-qcom-2-license ${PN}-linaro-license"
+RDEPENDS:${PN}-qcom-nord-adreno = "${PN}-qcom-license"
+RDEPENDS:${PN}-qcom-nord-audio = "${PN}-qcom-2-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-nord-compute = "${PN}-qcom-2-license"
 RDEPENDS:${PN}-qcom-nord-qupv3fw = "${PN}-qcom-license"
+RDEPENDS:${PN}-qcom-nord-soccp = "${PN}-qcom-2-license"
 RDEPENDS:${PN}-qcom-qcm2290-adreno = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-qcm2290-audio = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-qcm2290-modem = "${PN}-qcom-license"
@@ -2871,7 +2965,7 @@ RDEPENDS:${PN}-qcom-sc8280xp-lenovo-x13s-adreno = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-sc8280xp-lenovo-x13s-compute = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-sc8280xp-lenovo-x13s-sensors = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-sc8280xp-lenovo-x13s-vpu = "${PN}-qcom-license"
-RDEPENDS:${PN}-qcom-sc8280xp-radxa-dragon-q8b-audio = "${PN}-qcom-license"
+RDEPENDS:${PN}-qcom-sc8280xp-radxa-dragon-q8b-audio = "${PN}-qcom-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-sdm845-adreno = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-sdm845-audio = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-sdm845-compute = "${PN}-qcom-license"
@@ -2915,6 +3009,7 @@ RDEPENDS:${PN}-qcom-x1e80100-dell-xps13-9345-adreno = "${PN}-dell-license ${PN}-
 RDEPENDS:${PN}-qcom-x1e80100-dell-xps13-9345-audio = "${PN}-dell-license ${PN}-linaro-license ${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-x1e80100-dell-xps13-9345-compute = "${PN}-dell-license ${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-x1e80100-hp-omnibook-x14-audio-tplg = "${PN}-linaro-license"
+RDEPENDS:${PN}-qcom-x1e80100-lenovo-ideacenter-mini-audio-tplg = "${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-x1e80100-lenovo-t14s-g6-adreno = "${PN}-qcom-license"
 RDEPENDS:${PN}-qcom-x1e80100-lenovo-t14s-g6-audio = "${PN}-qcom-license ${PN}-linaro-license"
 RDEPENDS:${PN}-qcom-x1e80100-lenovo-t14s-g6-compute = "${PN}-qcom-license"
@@ -2938,7 +3033,10 @@ LICENSE:${PN}-mellanox = "LicenseRef-Firmware-WHENCE"
 FILES:${PN}-mellanox = "${firmwaredir}/mellanox"
 
 LICENSE:${PN}-mm8108 = "LicenseRef-Firmware-morsemicro"
-FILES:${PN}-mm8108 = "${firmwaredir}/morsemicro/mm81x/v56/*"
+FILES:${PN}-mm8108 = " \
+    ${firmwaredir}/morsemicro/mm81x/v56/* \
+    ${firmwaredir}/morsemicro/mm81x/v57/* \
+"
 
 LICENSE:${PN}-prestera = "LicenseRef-Firmware-Marvell"
 FILES:${PN}-prestera = "${firmwaredir}/mrvl/prestera"
@@ -2979,6 +3077,13 @@ LICENSE:${PN}-ish-nvlh = "LicenseRef-Firmware-intel"
 FILES:${PN}-ish-nvlh = "${firmwaredir}/intel/ish/ish_nvlh.bin*"
 RDEPENDS:${PN}-ish-nvlh = "${PN}-intel-license"
 
+LICENSE:${PN}-ish-nvlh-39ceeaf8 = "LicenseRef-Firmware-dell"
+FILES:${PN}-ish-nvlh-39ceeaf8 =  "\
+    ${firmwaredir}/intel/ish/ish_nvlh_39ceeaf8.bin* \
+    ${firmwaredir}/dell/ish/ish_nvlh_39ceeaf8_*.bin* \
+"
+RDEPENDS:${PN}-ish-nvlh-39ceeaf8 = "${PN}-dell-license"
+
 LICENSE:${PN}-ish-nvls = "LicenseRef-Firmware-intel"
 FILES:${PN}-ish-nvls = "${firmwaredir}/intel/ish/ish_nvls.bin*"
 RDEPENDS:${PN}-ish-nvls = "${PN}-intel-license"
@@ -3014,6 +3119,7 @@ RDEPENDS:${PN}-ish-lnlm-12128606 = "${PN}-hp-license"
 
 LICENSE:${PN}-ish-ptl-12128606 = "LicenseRef-Firmware-HP"
 FILES:${PN}-ish-ptl-12128606 = " \
+    ${firmwaredir}/intel/ish/ish_ptl_12128606_*.bin* \
     ${firmwaredir}/HP/ish/ish_ptl_12128606_*.bin* \
 "
 RDEPENDS:${PN}-ish-ptl-12128606 = "${PN}-hp-license"
@@ -3102,6 +3208,24 @@ FILES:${PN}-ccp = " \
 "
 RDEPENDS:${PN}-ccp = "${PN}-amd-sev-license"
 
+# For amd-sof - AMD ACP 7.F Sound Open Firmware
+LICENSE:${PN}-amdsof-license = "LicenseRef-Firmware-amdsof"
+FILES:${PN}-amdsof-license = "${firmwaredir}/LICENSE.amdsof"
+
+LICENSE:${PN}-amdsof-acp7f = "LicenseRef-Firmware-amdsof"
+FILES:${PN}-amdsof-acp7f = " \
+    ${firmwaredir}/amd/sof/sof-acp7f*bin* \
+    ${firmwaredir}/amd/sof/acp7f/v1.0/sof-acp7f*bin* \
+"
+RDEPENDS:${PN}-amdsof-acp7f = "${PN}-amdsof-license"
+
+LICENSE:${PN}-amdsof-acp7f-tplg = "LicenseRef-Firmware-amdsof"
+FILES:${PN}-amdsof-acp7f-tplg = " \
+    ${firmwaredir}/amd/sof-tplg/sof-acp7f*tplg* \
+    ${firmwaredir}/amd/sof-tplg/acp7f/v1.0/sof-acp7f*tplg* \
+"
+RDEPENDS:${PN}-amdsof-acp7f-tplg = "${PN}-amdsof-license"
+
 # For amdxdna - AMD Inference processor
 LICENSE:${PN}-amdnpu-license = "LicenseRef-Firmware-amdnpu"
 FILES:${PN}-amdnpu-license = "${firmwaredir}/LICENSE.amdnpu"
@@ -3146,6 +3270,14 @@ LICENSE:${PN}-as102 = "LicenseRef-Firmware-Abilis"
 FILES:${PN}-as102 = "${firmwaredir}/as102_data*_st.hex*"
 RDEPENDS:${PN}-as102 = "${PN}-abilis-license"
 
+# For awinic - awinic SmartK AMP driver
+LICENSE:${PN}-awinic = "LicenseRef-Firmware-awinic"
+FILES:${PN}-awinic = "${firmwaredir}/LICENCE.awinic"
+
+${PN}-awinic-aw88399 = "LicenseRef-Firmware-awinic"
+FILES:${PN}-awinic-aw88399 = "${firmwaredir}/awinic/aw88399_acf_*bin*"
+RDEPENDS:${PN}-awinic-aw88399 = "${PN}-awinic-license"
+
 # For wilc1000 - Atmel 802.11n WLAN driver for WILC1000 & WILC3000
 LICENSE:${PN}-atmel-license = "LicenseRef-Firmware-atmel"
 FILES:${PN}-atmel-license = "${firmwaredir}/LICENSE.atmel"
@@ -3426,11 +3558,6 @@ LICENSE:${PN}-atomisp = "LicenseRef-Firmware-ivsc"
 FILES:${PN}-atomisp = "${firmwaredir}/intel/ipu/shisp_240*"
 RDEPENDS:${PN}-atomisp = "${PN}-ivsc-license"
 
-# For intel-ipu6-isys - Intel IPU6 (Image Processing Unit 6) driver
-LICENSE:${PN}-intel-ipu6-isys = "LicenseRef-Firmware-ivsc"
-FILES:${PN}-intel-ipu6-isys = "${firmwaredir}/intel/ipu/ipu6*"
-RDEPENDS:${PN}-intel-ipu6-isys = "${PN}-ivsc-license"
-
 # For mei-vsc-hw - Intel Visual Sensing Controller
 LICENSE:${PN}-mei-vsc-hw = "LicenseRef-Firmware-ivsc"
 FILES:${PN}-mei-vsc-hw = "${firmwaredir}/intel/vsc/*"
@@ -3448,11 +3575,21 @@ FILES:${PN}-ipu3-imgu = " \
 "
 RDEPENDS:${PN}-ipu3-imgu = "${PN}-ipu3-firmware-license"
 
+# For intel-ipu6-isys - Intel IPU6 (Image Processing Unit 6) driver
+LICENSE:${PN}-intel-ipu6-isys = "LicenseRef-Firmware-ivsc"
+FILES:${PN}-intel-ipu6-isys = "${firmwaredir}/intel/ipu/ipu6*"
+RDEPENDS:${PN}-intel-ipu6-isys = "${PN}-ivsc-license"
+
 # For intel-ipu7-isys - Intel IPU7 (Image Processing Unit 7) driver
 LICENSE:${PN}-intel-ipu7-isys = "LicenseRef-Firmware-intel"
 FILES:${PN}-intel-ipu7-isys = "${firmwaredir}/intel/ipu/ipu7*"
 RDEPENDS:${PN}-intel-ipu7-isys = "${PN}-intel-license"
 
+# For intel-ipu8-isys - Intel IPU8 (Image Processing Unit 8) driver
+LICENSE:${PN}-intel-ipu8-isys = "LicenseRef-Firmware-intel"
+FILES:${PN}-intel-ipu8-isys = "${firmwaredir}/intel/ipu/ipu8*"
+RDEPENDS:${PN}-intel-ipu8-isys = "${PN}-intel-license"
+
 # For intel_vpu - Intel NPU driver
 LICENSE:${PN}-intel-vpu-license = "LicenseRef-Firmware-intel-vpu"
 FILES:${PN}-intel-vpu-license = "${firmwaredir}/LICENSE.intel_vpu"
@@ -3703,6 +3840,7 @@ LICENSE:${PN} = "\
     AND LicenseRef-Firmware-amlogic \
     AND LicenseRef-Firmware-amlogic-vdec \
     AND LicenseRef-Firmware-atmel \
+    AND LicenseRef-Firmware-awinic \
     AND LicenseRef-Firmware-bfa \
     AND LicenseRef-Firmware-bmi260 \
     AND LicenseRef-Firmware-bnx2 \
-- 
2.34.1



````
