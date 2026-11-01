/* The Ark engine for Kilowallet iOS: the Paperclip/bark wallet daemon and its REST API, run
 * inside the app on 127.0.0.1 behind a random bearer token (see src/lib.rs). */
#ifndef KILOMBINO_ARK_H
#define KILOMBINO_ARK_H

#include <stdint.h>

/* Starts the engine; returns the bearer token, or "ERR:<message>". `mnemonic` is NULL while the
 * app has no Ark wallet yet; `passphrase` is "" (or NULL) for none. Free the result with
 * kilombino_ark_free. Blocking: never call it on the main thread. */
char *kilombino_ark_start(const char *datadir, uint16_t port, const char *mnemonic, const char *passphrase);

/* Stops the engine (waits for a clean shutdown). */
void kilombino_ark_stop(void);

void kilombino_ark_free(char *s);

#endif
