/* A program that opens nothing and draws nothing, run by the driver exactly
 * as an engine's is: its pass measures what starting a process per file
 * costs (the driver, taskset, timeout, exec), the floor under every engine's
 * time and energy over a corpus. */
#include <stdio.h>

int main(int argc, char **argv) {
  (void)argc;
  (void)argv;
  printf("open\t0\t0\ndone\n");
  return 0;
}
