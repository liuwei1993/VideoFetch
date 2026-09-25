import re

from yt_dlp.extractor.common import InfoExtractor
from yt_dlp.utils import ExtractorError


class MissAVIE(InfoExtractor):
    IE_NAME = 'missav'
    _VALID_URL = (
        r'(?i)https?://(?:www\.)?missav\.(?:ws|com|ai)/'
        r'(?:dm\d+/)?'
        r'(?:[a-z]{2,3}/)?(?P<id>[\w-]+)/?(?:[?#].*)?$'
    )
    _LANGS = {
        'cn', 'en', 'ja', 'ko', 'ms', 'th', 'de', 'fr', 'vi', 'id', 'fil', 'pt',
    }
    _PAGE_HOST = 'https://missav.ai'
    _LISTING_IDS = {
        'makers', 'actresses', 'genres', 'articles', 'ads',
        'history', 'contact', 'chinese-subtitle', 'ranking', 'fc2',
    }

    def _real_extract(self, url):
        video_id = self._match_id(url)
        if (
            re.fullmatch(r'(?i)dm\d+', video_id)
            or video_id.lower() in self._LISTING_IDS
            or video_id.lower() in self._LANGS
        ):
            raise ExtractorError('Not a MissAV single video URL', expected=True)

        # missav.ws is often behind a Cloudflare challenge; .ai serves the same pages.
        # Drop the /dmNNN/ mirror prefix so the canonical watch path is requested.
        # The app passes `--impersonate Safari-18.0`; Chrome's fingerprint is blocked.
        page_url = re.sub(
            r'(?i)^https?://(?:www\.)?missav\.(?:ws|com|ai)',
            self._PAGE_HOST,
            url,
        )
        page_url = re.sub(r'(?i)(/)dm\d+/', r'\1', page_url, count=1)
        webpage = self._download_webpage(page_url, video_id)
        m3u8_url = self._playlist_from_packed(webpage)
        headers = {
            'Referer': f'{self._PAGE_HOST}/',
            'Origin': self._PAGE_HOST,
        }
        formats = self._extract_m3u8_formats(
            m3u8_url, video_id, 'mp4', m3u8_id='hls', headers=headers,
        )
        return {
            'id': video_id,
            'title': self._og_search_title(webpage),
            'description': self._og_search_description(webpage, default=''),
            'thumbnail': self._og_search_thumbnail(webpage, default=None),
            'formats': formats,
            'http_headers': headers,
            'age_limit': 18,
        }

    def _playlist_from_packed(self, webpage):
        m = re.search(r"'((?:m3u8\|)[^']+)'\.split\('\|'\)", webpage)
        if not m:
            raise ExtractorError('Unable to extract MissAV packed m3u8')
        words = m.group(1).split('|')
        if not words or words[0] != 'm3u8' or 'https' not in words:
            raise ExtractorError('Unexpected MissAV packed payload')
        https_index = words.index('https')
        if https_index < 7:
            raise ExtractorError('Unexpected MissAV packed domain')
        uuid = '-'.join(reversed(words[1:6]))
        domain = '.'.join(reversed(words[6:https_index]))
        return f'https://{domain}/{uuid}/playlist.m3u8'
