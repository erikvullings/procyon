const MICROSOFT_AUTHORIZATION_ORIGIN = 'https://login.microsoftonline.com';
const MICROSOFT_AUTHORIZATION_PATH = '/common/oauth2/v2.0/authorize';

/** Rejects unexpected schemes/hosts before an adapter opens a backend-provided authorization URL. */
export function trustedOneDriveAuthorizationUrl(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error('The Microsoft authorization URL is invalid.');
  }
  if (
    url.origin !== MICROSOFT_AUTHORIZATION_ORIGIN ||
    url.pathname !== MICROSOFT_AUTHORIZATION_PATH ||
    url.username.length > 0 ||
    url.password.length > 0
  ) {
    throw new Error('The Microsoft authorization URL is not trusted.');
  }
  return url.href;
}
