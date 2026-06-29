import { readdirSync, readFileSync } from 'node:fs';
import { extname, join } from 'node:path';

const blogDir = 'site/src/content/blog';
const markdownFiles = readdirSync(blogDir)
  .filter((name) => ['.md', '.mdx'].includes(extname(name)))
  .sort();

const missingStatus = markdownFiles.filter((name) => {
  const content = readFileSync(join(blogDir, name), 'utf8');
  const frontmatterMatch = content.match(/^---\n([\s\S]*?)\n---/);

  return !frontmatterMatch || !/^status:\s*["']?(draft|published)["']?\s*$/m.test(frontmatterMatch[1]);
});

if (missingStatus.length > 0) {
  throw new Error(`Blog posts must declare status: ${missingStatus.join(', ')}`);
}

const contentConfig = readFileSync('site/src/content.config.ts', 'utf8');

if (/status:[\s\S]*default\(['"]published['"]\)/m.test(contentConfig)) {
  throw new Error('Blog status must not default to published');
}
