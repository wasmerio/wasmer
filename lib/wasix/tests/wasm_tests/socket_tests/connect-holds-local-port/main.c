/*
 * Verify that a connected TCP socket keeps its local port reserved.
 *
 * POSIX / Linux behaviour:
 *   - A socket that has completed connect() holds its local (ephemeral) port
 *     for its entire lifetime.
 *   - Attempting to bind a *different* socket to the same local address while
 *     the first socket is still connected must fail with EADDRINUSE.
 *   - After the connected socket is closed the port is released and a new
 *     bind() to the same address must succeed.
 *
 * Steps
 *   1. Create a server socket and listen on 127.0.0.1:0.
 *   2. Bind a client socket to 127.0.0.1:0 (ephemeral) and connect to server.
 *   3. Record the client's local address via getsockname.
 *   4. Try to bind a third socket to that exact local address → EADDRINUSE.
 *   5. Close the connected client socket.
 *   6. Bind a third socket to the same address again → must succeed.
 */
#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(void) {
  /* ---- step 1: server ---- */
  int server = socket(AF_INET, SOCK_STREAM, 0);
  if (server < 0) {
    perror("socket(server)");
    return 1;
  }

  int one = 1;
  setsockopt(server, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));

  struct sockaddr_in srv_addr;
  memset(&srv_addr, 0, sizeof(srv_addr));
  srv_addr.sin_family = AF_INET;
  srv_addr.sin_port = htons(0);
  inet_pton(AF_INET, "127.0.0.1", &srv_addr.sin_addr);

  if (bind(server, (struct sockaddr*)&srv_addr, sizeof(srv_addr)) < 0) {
    perror("bind(server)");
    close(server);
    return 1;
  }
  if (listen(server, 1) < 0) {
    perror("listen");
    close(server);
    return 1;
  }

  socklen_t srv_len = sizeof(srv_addr);
  getsockname(server, (struct sockaddr*)&srv_addr, &srv_len);

  /* ---- step 2: client — bind to ephemeral port then connect ---- */
  int client = socket(AF_INET, SOCK_STREAM, 0);
  if (client < 0) {
    perror("socket(client)");
    close(server);
    return 1;
  }

  struct sockaddr_in cli_bind;
  memset(&cli_bind, 0, sizeof(cli_bind));
  cli_bind.sin_family = AF_INET;
  cli_bind.sin_port = htons(0);
  inet_pton(AF_INET, "127.0.0.1", &cli_bind.sin_addr);

  if (bind(client, (struct sockaddr*)&cli_bind, sizeof(cli_bind)) < 0) {
    perror("bind(client)");
    close(server);
    close(client);
    return 1;
  }
  if (connect(client, (struct sockaddr*)&srv_addr, sizeof(srv_addr)) < 0) {
    perror("connect");
    close(server);
    close(client);
    return 1;
  }

  /* ---- step 3: record the client's local port ---- */
  struct sockaddr_in cli_local;
  socklen_t cli_len = sizeof(cli_local);
  memset(&cli_local, 0, sizeof(cli_local));
  if (getsockname(client, (struct sockaddr*)&cli_local, &cli_len) < 0) {
    perror("getsockname(client)");
    close(server);
    close(client);
    return 1;
  }
  int cli_port = (int)ntohs(cli_local.sin_port);
  if (cli_port == 0) {
    fprintf(stderr, "client local port is 0 after connect\n");
    close(server);
    close(client);
    return 1;
  }

  /* ---- step 4: rebind to same port while client is still connected ---- */
  int probe = socket(AF_INET, SOCK_STREAM, 0);
  if (probe < 0) {
    perror("socket(probe)");
    close(server);
    close(client);
    return 1;
  }

  if (bind(probe, (struct sockaddr*)&cli_local, sizeof(cli_local)) == 0) {
    fprintf(stderr,
            "bind to port %d succeeded while client socket is still connected "
            "(expected EADDRINUSE)\n",
            cli_port);
    close(probe);
    close(server);
    close(client);
    return 1;
  }
  if (errno != EADDRINUSE) {
    fprintf(stderr,
            "bind to port %d failed with errno %d (%s), expected EADDRINUSE\n",
            cli_port, errno, strerror(errno));
    close(probe);
    close(server);
    close(client);
    return 1;
  }
  close(probe);

  /* ---- step 5: close the connected client ---- */
  close(client);

  /* ---- step 6: now the port must be available again ---- */
  int probe2 = socket(AF_INET, SOCK_STREAM, 0);
  if (probe2 < 0) {
    perror("socket(probe2)");
    close(server);
    return 1;
  }

  if (bind(probe2, (struct sockaddr*)&cli_local, sizeof(cli_local)) < 0) {
    fprintf(stderr,
            "bind to port %d failed after client socket was closed: %s\n",
            cli_port, strerror(errno));
    close(probe2);
    close(server);
    return 1;
  }
  close(probe2);
  close(server);

  printf("connected socket holds its local port\n");
  return 0;
}
