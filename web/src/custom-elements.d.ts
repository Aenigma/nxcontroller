import type { JSX } from "preact";

declare module "preact" {
  namespace JSX {
    interface IntrinsicElements {
      /** ESP Web Tools' install button (registered by esp-web-tools/dist/web/install-button.js). */
      "esp-web-install-button": JSX.HTMLAttributes<HTMLElement> & { manifest?: string };
    }
  }
}
