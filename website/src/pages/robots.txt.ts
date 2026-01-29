import type { APIRoute } from 'astro';

export const GET: APIRoute = ({ site }) => {
  const siteUrl = site?.toString().replace(/\/$/, '') ?? '';

  const robotsTxt = `# robots.txt
User-agent: *
Allow: /

# Sitemap location
Sitemap: ${siteUrl}/sitemap-index.xml
`;

  return new Response(robotsTxt, {
    headers: {
      'Content-Type': 'text/plain; charset=utf-8',
    },
  });
};
