#include <stdio.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <errno.h>

int main() {
    pid_t pid, pgid;
    
    printf("Test sys_getpgid and sys_setpgid\n");
    
    // Test getpgid(0) - get current process group ID
    pgid = getpgid(0);
    printf("Current PGID: %d\n", pgid);
    
    // Test getpgid(getpid()) - should be same as above
    pid = getpid();
    pgid = getpgid(pid);
    printf("PGID of PID %d: %d\n", pid, pgid);
    
    // Test setpgid(0, 0) - make current process its own group leader
    if (setpgid(0, 0) == 0) {
        printf("Successfully created new process group\n");
        pgid = getpgid(0);
        printf("New PGID: %d (should equal PID %d)\n", pgid, pid);
        
        if (pgid == pid) {
            printf("TEST PASSED: PGID equals PID after setpgid(0, 0)\n");
        } else {
            printf("TEST FAILED: PGID should equal PID\n");
        }
    } else {
        perror("setpgid(0, 0) failed");
        return 1;
    }
    
    // Test setpgid with same group (should succeed)
    if (setpgid(0, pgid) == 0) {
        printf("setpgid with same group succeeded (as expected)\n");
    } else {
        perror("setpgid with same group failed");
    }
    
    printf("All tests completed\n");
    return 0;
}