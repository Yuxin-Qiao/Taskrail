# Design QA

- Source visual truth: the latest center-panel feedback screenshot, plus the original supplied topology reference.
- Implementation screenshot: `taskrail-topology-final.png` (temporary local capture).
- Viewport: source feedback crop 956 × 618 px; implementation rendered at the README display width of 960 px
- Implementation dimensions: 960 × 540 px; SVG viewBox is 1600 × 900; density normalization is not required
- State: static README hero, dark high-contrast visual direction

## Comparison

The source feedback crop and the implementation were opened and compared as the same focused center-panel review. The implementation keeps the requested dark visual language and makes the product's actual positioning explicit: open-source tools on the left, Taskrail as the local control plane in the center, and scheduling, policy, result history, and inspection on the right.

The focused comparison region was the three-column topology. The final render centers the Taskrail hub at x=800 in the 1600px viewBox, aligns the five left connectors and four right connectors to card centers, and reduces the unused vertical gap above the topology. Five concrete inputs remain visible—Mole, Homebrew, restic, rclone, and Local + ChatGPT—with the right side showing “Scheduled cleanup”, “Safe execution”, “Every result recorded”, and “One place to inspect”.

The five tool cards use the corresponding upstream brand assets: Mole's mouse mark, Homebrew's mug, restic's mascot, rclone's sync symbol, and Taskrail's own control-plane mark. The source files are kept under `docs/assets/integrations/`; the topology embeds them as data URIs so the single README SVG does not depend on fragile relative image paths.

## Required fidelity surfaces

- Fonts and typography: uses system sans-serif for product and tool names, and a monospace stack for technical labels. Headings and tool names remain readable at 960 px.
- Spacing and layout: preserves a centered hero and three-column flow while allocating enough width to show actual tool names and capabilities without overlap.
- Colors and tokens: navy is the control-plane surface; blue identifies connected tools and inspection; cyan indicates immediate execution; lime indicates scheduling/success; coral indicates audit and policy-sensitive work.
- Clarity and density: the central card is widened to 680px, the four-stage operating loop is explicit, tool/outcome typography is increased, and low-contrast secondary labels use a brighter `#DCE4FF` token.
- Image quality and asset fidelity: uses the upstream integration marks in their original proportions, with raster assets embedded at card size and vector assets preserved as SVG data. Logo, topology, and standalone SVG assets pass XML validation.
- Copy and content: uses only integrations present in the repository's semantic integration layer: Mole, Homebrew, restic, rclone, local jobs, ChatGPT, and the documented policy/audit concepts.
- Links: English and Chinese README headers include direct links to the Mole, Homebrew, restic, and rclone repositories.

## Findings

## Iteration history

- Previous pass finding: [P1] the center felt off-axis and too sparse at README scale; the hub and surrounding cards did not read as one compact control-plane composition.
- Fix: moved the topology into a symmetric 1600px grid, widened the hub to 680px, aligned connectors to card centers, reduced the header-to-topology gap, and replaced the sparse two-row capability list with a readable four-stage `DISCOVER → SCHEDULE → EXECUTE → INSPECT` strip.
- Post-fix evidence: the final topology render; the hub is centered, the card rows share a common rhythm, and labels remain readable at 960px output.

No actionable P0/P1/P2 findings remain. The original bright screenshot is intentionally superseded by the new dark ecosystem composition. Each integration card has a recognizable upstream mark while the restrained card labels keep the diagram readable at README width.

## Implementation checklist

- [x] Replaced generic “inputs / outputs” with concrete open-source automation tools.
- [x] Made Mole cleanup a visible scheduled outcome.
- [x] Made Taskrail's registry, scheduler, executor, policy, and audit role visible in the center.
- [x] Added clickable upstream links for Mole, Homebrew, restic, and rclone.
- [x] Added the corresponding upstream marks for Mole, Homebrew, restic, and rclone.
- [x] Embedded the icon assets in the topology SVG so they render without external relative-path lookups.
- [x] Centered the hub and aligned all input/output connectors to the card rows.
- [x] Increased information density and contrast in the hub and supporting cards.
- [x] Synchronized English and Chinese README alt text and tool links.
- [x] `git diff --check` passed.
- [x] `xmllint --noout docs/assets/taskrail-mark.svg docs/assets/taskrail-topology.svg docs/assets/integrations/homebrew.svg docs/assets/integrations/rclone.svg` passed.

final result: passed
