# StudyBites mobile UI guide

The mobile layout targets real phones at 320, 360, 390 and 412 pixels wide,
including short screens, landscape, safe areas and large system text. Desktop
layout remains unchanged above the existing breakpoints.

## Screen primitive

Use [`Screen`](../src/components/Screen.tsx) for every new screen and dialog.
It is a column with:

- an optional `header`;
- a scrolling `body` with `min-height: 0`, contained overscroll and no fixed
  content height;
- an optional `footer` for the primary action.

Example:

```tsx
<Screen
  header={<h1>Study</h1>}
  footer={<button className="btn btn--primary">Continue</button>}
>
  <p>Scrollable content.</p>
</Screen>
```

The footer has a surface background, a top border, safe-area padding and a
52px primary button. In the phone app shell, use `screen--nav` so it sits above
the bottom navigation. Existing dialogs use `Modal`, which supplies this
primitive and pins confirm/cancel actions automatically.

## Layout rules

- Keep the main action bottom-anchored through the `Screen` footer.
- Let the body fill available space and scroll internally; do not use bare
  `100vh` for full-height layouts. Use the primitive's `100dvh` plus fallback.
- Keep horizontal overflow hidden at the page level. Wrap long words and let
  long lists or tables scroll in their own container.
- Keep controls at least 44x44px with 8px spacing; use at least 16px text in
  inputs on phones.
- Respect `env(safe-area-inset-*)`, light/dark tokens and reduced motion.
- Use the existing tokens for spacing, type and surfaces. Avoid one-off mobile
  dimensions when a shared rule can solve the problem.
- Test short heights (568, 640, 800), landscape and the largest system font
  size. The focused field must remain visible above the keyboard.

## Device checklist

- Agreement fills the screen and Accept is at the bottom.
- Activate stays above the keyboard.
- Study reveal and grade buttons stay at the bottom.
- CBT navigation and submit stay at the bottom.
- Every dialog keeps confirm at the bottom.
- Settings scrolls without content hiding under bottom navigation.
- Landscape works.
- Large system font still keeps text and actions usable.

Visual results must be verified on a real Android device; code checks alone
cannot prove these layout outcomes.
