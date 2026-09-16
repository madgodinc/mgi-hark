// Whole-word signs. In Russian Sign Language most words have their own sign
// ("дела" is one sign) while short function words and names are spelled
// letter by letter ("как" is К-А-К). The player looks a word up here first
// and falls back to the manual alphabet when it is missing.
//
// Format: lowercase word -> keyframes, the same pose format as the alphabets
// (see ../hand.js). A sign that needs two hands or touches the face can only
// be approximated by one floating hand; such entries say so.
//
// DRAFTS, not yet checked by a signer. Sources: short descriptions in
// thegirl.ru ("8 фраз на языке жестов") and baby.ru ("Спасибо на языке жестов
// и 5 других базовых фраз").

const S = [0, 0, 0, 0];
const F = [0, 88, 98, 62];
const fist = { index: F, middle: F, ring: F, pinky: F, thumb: [42, 12, 22, 18], touch: "middle-finger-phalanx-intermediate" };
const open = { thumb: [0, 20, 0, 0] };

const move = (pose, ...deltas) => [pose, ...deltas.map((d) => ({ ...pose, ...d }))];

export const WORDS = {
  ru: {
    // Open palm waved side to side.
    привет: move(open, { rot: [0, 0, 22] }, { rot: [0, 0, -18] }, { rot: [0, 0, 22] }, { rot: [0, 0, 0] }),
    // Index and middle straight, then folded into the fist in one movement.
    да: [
      { ...fist, index: S, middle: S, touch: "ring-finger-phalanx-intermediate" },
      { ...fist, index: [0, 40, 40, 20], middle: [0, 40, 40, 20], touch: "ring-finger-phalanx-intermediate" },
      fist,
    ],
    // Like the greeting, but the palm moves once, away from the body.
    нет: move(open, { pos: [-0.07, 0, 0], rot: [0, 0, -10] }),
    // Fist touches the forehead, then the knuckles touch the chin. No face in
    // the model, so only the top-to-bottom path is shown.
    спасибо: move({ ...fist, rot: [0, 70, 0], pos: [0, 0.06, 0] }, { pos: [0, 0.06, 0.01] }, { pos: [0, -0.04, 0.01] }),
    // Thumb up.
    хорошо: move({ ...fist, thumb: [0, 50, 0, 0], touch: undefined, rot: [0, 0, 90] }, { pos: [0, 0.015, 0] }),
    // Little finger up.
    плохо: move({ ...fist, pinky: S, touch: "index-finger-phalanx-intermediate" }, { rot: [0, 0, -10] }),
  },
  en: {},
};
