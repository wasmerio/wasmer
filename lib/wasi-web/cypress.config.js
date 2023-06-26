import { defineConfig } from "cypress";
import { initPlugin } from "@frsource/cypress-plugin-visual-regression-diff/plugins";

export default defineConfig({
  e2e: {
    baseUrl: 'http://localhost:9000',
    experimentalStudio: true,
    setupNodeEvents(on, config) {
      initPlugin(on, config);

      on('after:spec', (spec, results) => {
        if (results && results.video) {
          // Do we have failures for any retry attempts?
          const failures = results.tests.some((test) =>
            test.attempts.some((attempt) => attempt.state === 'failed')
          )
          if (!failures) {
            // delete the video if the spec passed and no tests retried
            fs.unlinkSync(results.video)
          }
        }
      })
    },
  },
  // Workaround for issues with SharedArrayBuffer and self.crossOriginIsolated
  chromeWebSecurity: false,
  trashAssetsBeforeRuns: true,
});
