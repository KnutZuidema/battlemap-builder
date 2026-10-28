/**
 * Shared element registry.
 *
 * The entry point caches every element once and publishes the locals here at the end of
 * `cacheDOMElements()`; view modules read them through `dom.<name>` instead of closing
 * over entry-point locals. Nothing in this module touches the document.
 */

export const dom = {};
