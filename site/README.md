# PGSandbox MCP Site

The Astro site keeps blog posts in `src/content/blog`. Each Markdown file uses
frontmatter validated by `src/content.config.ts`; the filename becomes the blog
route under `/blog/{slug}/`.

The production deploy workflow builds `site/dist` in GitHub Actions, then
deploys that prebuilt static output to CapRover. The CapRover Docker image only
serves `dist` through nginx.
