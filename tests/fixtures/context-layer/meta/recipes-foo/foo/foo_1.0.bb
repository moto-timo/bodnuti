SUMMARY = "Example recipe for the layer 2 context tests"
LICENSE = "MIT"

require foo.inc
require ${BPN}-common.inc
require ../../../../../etc/passwd
include /etc/passwd
require link.inc

inherit myclass ${EXTRA_CLASS}
