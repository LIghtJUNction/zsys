#define _GNU_SOURCE
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <fcntl.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <netinet/in.h>
#include <sys/ioctl.h>
#include <sys/statvfs.h>
#include <sys/syscall.h>
#include <unistd.h>

/* Match every manually declared libc record used by the Linux backend. */
_Static_assert(sizeof(struct sockaddr_in) == 16, "sockaddr_in size");
_Static_assert(offsetof(struct sockaddr_in, sin_addr) == 4, "IPv4 offset");
_Static_assert(sizeof(struct sockaddr_in6) == 28, "sockaddr_in6 size");
_Static_assert(offsetof(struct sockaddr_in6, sin6_addr) == 8, "IPv6 offset");
_Static_assert(offsetof(struct sockaddr_in6, sin6_scope_id) == 24, "scope offset");
_Static_assert(offsetof(struct ifreq, ifr_ifru) == 16, "ifreq union offset");
_Static_assert(sizeof(struct ifreq) == (sizeof(void *) == 8 ? 40 : 32), "ifreq size");
_Static_assert(_Alignof(struct ifreq) == _Alignof(void *), "ifreq alignment");
_Static_assert(offsetof(struct ifconf, ifc_ifcu) == (sizeof(void *) == 8 ? 8 : 4), "ifconf pointer offset");
_Static_assert(sizeof(struct ifaddrs) == (sizeof(void *) == 8 ? 56 : 28), "ifaddrs size");
_Static_assert(offsetof(struct ifaddrs, ifa_addr) == (sizeof(void *) == 8 ? 24 : 12), "ifaddrs sockaddr offset");
_Static_assert(offsetof(struct ifaddrs, ifa_data) == (sizeof(void *) == 8 ? 48 : 24), "ifaddrs data offset");
_Static_assert(O_CLOEXEC == 0x80000, "O_CLOEXEC constant");
_Static_assert(SOCK_CLOEXEC == 0x80000, "SOCK_CLOEXEC constant");
_Static_assert(_SC_CLK_TCK == 2, "_SC_CLK_TCK constant");
_Static_assert(_SC_PAGESIZE == 30, "_SC_PAGESIZE constant");
_Static_assert(SIOCGIFCONF == 0x8912 && SIOCGIFFLAGS == 0x8913, "interface ioctl constants");
_Static_assert(SIOCGIFNETMASK == 0x891b && SIOCGIFMTU == 0x8921, "interface ioctl constants");
_Static_assert(SIOCGIFHWADDR == 0x8927 && SIOCGIFINDEX == 0x8933, "interface ioctl constants");
#if UINTPTR_MAX == UINT64_MAX
_Static_assert(sizeof(struct statvfs) == 112, "statvfs size");
_Static_assert(_Alignof(struct statvfs) == 8, "statvfs alignment");
_Static_assert(offsetof(struct statvfs, f_bavail) == 32, "available block offset");
_Static_assert(offsetof(struct statvfs, f_flag) == 72, "statvfs flags offset");
_Static_assert(offsetof(struct statvfs, f_namemax) == 80, "statvfs last value offset");
#endif
#if defined(__x86_64__) && !defined(__ILP32__)
_Static_assert(SYS_getdents64 == 217, "getdents64 syscall number");
#elif defined(__aarch64__)
_Static_assert(SYS_getdents64 == 61, "getdents64 syscall number");
#endif
int main(void) {
    puts("Linux ABI layouts and constants match native headers");
    return 0;
}
