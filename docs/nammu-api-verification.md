# NAMUH PLUG 연금계좌 검증 절차 (read-only probe)

> 이 문서는 나무증권(NH투자증권) API 플러그로 연금계좌 접근 **가능 여부만** 확인하는
> 절차다. 주문·정정·취소·예약 endpoint는 절대 호출하지 않는다. **이 도구는 자동주문·
> 수익보장·투자자문이 아니다.**

## 전제 조건 (Prerequisites

1. NH투자증권 나무증권 앱 계좌 + [nhplug.com](https://www.nhplug.com) API 사용신청
   (앱키 `NAMU_APP_KEY`, 시크릿 `NAMU_APP_SECRET` 발급; 1년 유효,
   3개월 무거래 시 자동해지 가능).
2. `cargo`(Rust 1.95.0, `rust-toolchain.toml` 고정) 빌드 가능 환경.
 VS Build
   Tools(VC++ workload) 필요 (Windows).
3. 실행 환경에 다음 **비밀 변수**를 설정 — 절대 커밋·로그에 노출 금지
   (Wealthfolio 암호화 비밀 저장소에서 꺼내 셸에 주입):

```powershell
$env:NAMU_API_BASE_URL = "https://api.nhplug.com:8443"   # N2 계정이면 별도 도메인
$env:NAMU_APP_KEY     = "발급받은 앱키"
$env:NAMU_APP_SECRET  = "발급받은 시크릿"
```

## 픽스처 모드 (네트워크 없음; 회귀 확인

```powershell
cargo run -p wealthfolio-server --bin verify_namu_account -- --account-id 12345678
```

기대 출력:

```text
Namu account ****5678 -> Supported (1 holdings)
```

## 라이브 모드 (운영 인증 사용

대상 계좌번호를 `--account-id`로 전달 (생략 시 `NAMU_ACCOUNT_ID` 환경변수 사용;
둘 다 없으면 계좌목록의 첫 번째 계좌):

```powershell
cargo run -p wealthfolio-server --bin verify_namu_account -- --live --account-id <11자리-계좌번호>
```

### 지원되는 결과 (Supported

```text
Namu account ****5678 -> Supported (N holdings)
```

→ 연금계좌 조회가 가능함을 의미. 이후 단계(ETF Universe, 리밸런싱 연결)에서
이 계좌의 잔고를 읽기 전용으로 동기화할 수 있다.

### 지원되지 않는 결과 (Unsupported / 오류

```text
Namu account ****5678 -> Unsupported: confirm pension-account coverage with NAMUH PLUG
```

→ 연금계좌는 PLUG 조회 API에서 지원되지 않거나 다른 계좌 유형일 가능성.
**API 공급자 확인 없이 우회하지 않는다** (사양 핵심 결정 참조).

그 밖에 `Unauthorized`(토큰/권한), `IncompleteResponse`(필드 누락),
`Transport`(HTTP 상태만) 오류가 뜨면 그 원인을 먼저 해결한다.


## 종료 코드

| 코드 | 의미 |
| --- | --- |
| `0` | Supported |
| `1` | Unsupported / Unauthorized / IncompleteResponse |
| `2` | Transport / 사용법 오류 |

## 안전 규칙 (No-order-permission

- 이 바이너리는 **SQLite 쓰기 트랜잭션을 절대 열지 않는다** — Wealthfolio DB에 아무것도 쓰지 않는다.

- 호출하는 경로는 토큰 `POST /oauth2/token`, 계좌목록 `GET /n2/acctinfo`,
  잔고 `POST /krstock/inquiry/v1/balance`, 현재가 `POST /krstock/quote/v1/currentPrice`
  뿐이다. `order|buy|sell|cancel|reserve` 경로를 포함한 어떤 주문
  endpoint도 호출하지 않는다 (테스트 `namu_adapter_test.rs::oauth_and_read_calls_are_read_only`가
  이를 기록·검증한다).
- 계좌번호는 마지막 4자리만 남기고 마스킹한다 (`****5678`).
- 라이브 결과는 날짜, 마스킹된 계좌 접미사, `Supported`/`Unsupported`,
  오류 코드**만** 기록한다 (예: `docs/nammu-api-verification-result.json`에 커밋 금지 —
  `.gitignore`의 `namu-live-result.json` 참조).