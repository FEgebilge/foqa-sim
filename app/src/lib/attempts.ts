import type { AttemptState } from "../types";

/** Started and not yet graded: Finish and Fly again apply (docs/challenge-protocol.md B5). */
export const OPEN_STATES: AttemptState[] = ["created", "placing", "flying", "approach", "rollout", "finished"];
