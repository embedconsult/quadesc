/* Task-local host fixture. Links unmodified pinned XCPlite; not an upstream demo. */
#include <a2l.h>
#include <xcplib.h>
#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
static volatile sig_atomic_t running = 1;
static void stop(int sig) { (void)sig; running = 0; }
static const struct { uint16_t period; uint16_t duty; } led = {1000,500}, other = {2000,250};
int main(int argc, char **argv) {
    assert(argc == 2);
    uint16_t port = (uint16_t)strtoul(argv[1],NULL,10);
    const uint8_t addr[4] = {127,0,0,1};
    signal(SIGTERM,stop); signal(SIGINT,stop); setbuf(stdout,NULL);
    XcpSetLogLevel(4);
    assert(XcpInit("p_open_reference", "T17-P-OPEN-88e3cbe", XCP_MODE_LOCAL|XCP_MODE_PERSISTENCE));
    assert(XcpEthServerInit(addr,port,false,32768));
    assert(A2lInit(addr,port,false,A2L_MODE_WRITE_ONCE|A2L_MODE_FINALIZE_ON_CONNECT));
    tXcpCalSegIndex a=XcpCreateCalSeg("led", &led,sizeof(led));
    tXcpCalSegIndex b=XcpCreateCalSeg("other", &other,sizeof(other));
    assert(a!=XCP_UNDEFINED_CALSEG && b!=XCP_UNDEFINED_CALSEG);
    A2lSetSegmentAddrMode(a,led);
    A2lCreateParameter(led.period,"period","ms",100,10000);
    A2lCreateParameter(led.duty,"duty","permille",0,1000);
    A2lSetSegmentAddrMode(b,other);
    A2lCreateParameter(other.period,"control period","ms",100,10000);
    A2lCreateParameter(other.duty,"control duty","permille",0,1000);
    for (unsigned i=0;i<2;i++) {
        tXcpCalSegIndex s=i?b:a;
        const uint16_t *p=(const uint16_t *)XcpLockCalSeg(s);
        printf("STARTUP %u %u %u\n",(unsigned)s,p[0],p[1]);
        XcpUnlockCalSeg(s);
    }
    puts("FIXTURE_READY loopback UDP");
    while(running) { struct timespec delay={0,10000000}; nanosleep(&delay,NULL); }
    XcpDisconnect(); XcpEthServerShutdown(); return 0;
}
