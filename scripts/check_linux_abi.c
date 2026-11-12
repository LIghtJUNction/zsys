/* Compile/run against native headers; no Rust libc crate is used. Linux LP64 only. */
#define _DEFAULT_SOURCE
#include <ifaddrs.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/statvfs.h>
#include <netinet/in.h>
#include <netpacket/packet.h>
#include <unistd.h>
_Static_assert(sizeof(void *) == 8, "this check targets LP64");
_Static_assert(sizeof(struct statvfs) == 112, "statvfs size");
_Static_assert(_Alignof(struct statvfs) == 8, "statvfs alignment");
_Static_assert(offsetof(struct statvfs, f_bavail) == 32, "statvfs bavail");
_Static_assert(offsetof(struct statvfs, f_flag) == 72, "statvfs flags");
_Static_assert(offsetof(struct statvfs, f_namemax) == 80, "statvfs namemax");
_Static_assert(sizeof(struct ifaddrs) == 56, "ifaddrs size");
_Static_assert(offsetof(struct ifaddrs, ifa_flags) == 16, "ifaddrs flags");
_Static_assert(offsetof(struct ifaddrs, ifa_addr) == 24, "ifaddrs address");
_Static_assert(sizeof(struct sockaddr_in) == 16, "IPv4 sockaddr");
_Static_assert(sizeof(struct sockaddr_in6) == 28, "IPv6 sockaddr");
_Static_assert(offsetof(struct sockaddr_in6, sin6_scope_id) == 24, "IPv6 scope");
_Static_assert(sizeof(struct sockaddr_ll) == 20, "link sockaddr");
_Static_assert(offsetof(struct sockaddr_ll, sll_addr) == 12, "link address");
_Static_assert(_SC_CLK_TCK == 2 && _SC_PAGESIZE == 30, "sysconf identifiers");
int main(void) { return sysconf(_SC_PAGESIZE) > 0 && sysconf(_SC_CLK_TCK) > 0 ? 0 : 1; }
