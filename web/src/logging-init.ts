// Side-effect module: installs the client logger before any other
// module gets a chance to throw. main.tsx imports this first so the
// global window error/unhandledrejection handlers are armed before
// React or any async fetch can run.
import { installClientLogger } from "./lib/logger";

installClientLogger();
