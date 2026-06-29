import type { APIRoute } from 'astro';
import { getPublishedBlogPosts } from '../lib/rowsetBlog';

const staticPages = [
  '/',
  '/docs/',
  '/docs/install/',
  '/docs/mcp-tools/',
  '/docs/architecture/',
  '/docs/homebrew/',
  '/blog/',
  '/changelog/'
];

function escapeXml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}

export const GET: APIRoute = async ({ site }) => {
  const baseUrl = site ?? new URL('https://pgsandbox-mcp.cap.gregagi.com');
  const posts = await getPublishedBlogPosts();
  const latestPost = posts[0];
  const pages = [
    ...staticPages.map((path) => ({ path, lastmod: latestPost?.updatedAt || latestPost?.publishedAt })),
    ...posts.map((post) => ({
      path: `/blog/${post.slug}/`,
      lastmod: post.updatedAt || post.publishedAt
    }))
  ];

  const urls = pages
    .map(({ path, lastmod }) => {
      const location = escapeXml(new URL(path, baseUrl).toString());
      const lastmodTag = lastmod ? `\n    <lastmod>${escapeXml(lastmod)}</lastmod>` : '';

      return `  <url>\n    <loc>${location}</loc>${lastmodTag}\n  </url>`;
    })
    .join('\n');

  return new Response(`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`, {
    headers: {
      'Content-Type': 'application/xml; charset=utf-8'
    }
  });
};
