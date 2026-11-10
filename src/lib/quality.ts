import type { Quality, QualityCapability, Track, TrackQualities } from './api';

export const QUALITY_ORDER: Quality[] = [
  'standard',
  'super',
  'lossless',
  'hires',
  'atmos',
  'master',
];

export interface QualityChoice {
  value: Quality;
  label: string;
  disabled: boolean;
  current: boolean;
}

function isAdvanced(q: Quality): boolean {
  return q === 'hires' || q === 'atmos' || q === 'master';
}

const FALLBACK_LABEL: Record<Quality, string> = {
  standard: '标准 128k',
  super: '高品质 320k',
  lossless: '无损 FLAC',
  hires: 'Hi-Res',
  atmos: '杜比全景声',
  master: '母带',
};

/**
 * 统一生成音质切换项。播放器菜单、详情页、设置页共用同一套判断，
 * 避免“这里能点、那里不能点”。不支持的只灰掉，不附加说明文字。
 */
export function qualityChoices(
  track: Track | null,
  caps: QualityCapability[],
  availability: TrackQualities | null,
  current: Quality
): QualityChoice[] {
  const capMap = new Map(caps.map((c) => [c.value, c]));
  const fromTrack = track?.available_qualities ?? [];
  const known = !!availability?.known || fromTrack.length > 0;
  const available = new Set<Quality>(
    availability?.known ? availability.available : fromTrack
  );

  return QUALITY_ORDER.map((value) => {
    const cap = capMap.get(value);
    let disabled = false;

    if (isAdvanced(value)) {
      disabled = true;
    } else if (known && !available.has(value)) {
      disabled = true;
    } else if (!known && value === 'lossless') {
      // 还没探测到 MINFO：先禁用无损，等 track_qualities 回来再放开
      disabled = true;
    }

    return {
      value,
      label: cap?.label ?? FALLBACK_LABEL[value],
      disabled,
      current: current === value,
    };
  });
}
