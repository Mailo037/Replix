import { untrack } from "svelte";
import { api } from "./api";

/**
 * The editor works in rounds. Its media work (filmstrip, waveforms, split tracks) belongs
 * to the round it was asked for in, and ending a round stops all of it at once: the editor
 * closed or the window was hidden. Starts at 1, as Replix drops work of round 0.
 */
export const editorRound = $state({ n: 1 });

/** Starts counting where Replix is, so a reloaded page never reuses an ended round. */
export async function syncEditorRound() {
  const first = await api.firstEditorRound();
  if (first > untrack(() => editorRound.n)) editorRound.n = first;
}

/** The round to ask for work in now. Untracked, so asking makes no effect depend on it. */
export const currentRound = () => untrack(() => editorRound.n);

/** Ends the current round and starts the next. */
export function endEditorRound() {
  const ended = currentRound();
  editorRound.n = ended + 1;
  api.endEditorRound(ended);
}
