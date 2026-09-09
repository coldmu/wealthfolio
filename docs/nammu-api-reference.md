# 나무증권 API 플러그(NAMUH PLUG) 사용 API 참조

어댑터(`crates/connect/src/broker/namu/`)가 호출하는 API 전체 목록과
요청·응답 형식. **이 문서의 모든 경로는 읽기 전용이며, 주문 endpoint는
어떤 것도 호출하지 않는다.** 실행 절차는
[`docs/nammu-api-verification.md`](nammu-api-verification.md) 참고.

## 개요

- **대상 서비스**: NH투자증권 나무증권 API 플러그(NAMUH PLUG) — HTTPS 기반 REST OpenAPI (`v1` 스펙)
- **자격증명 발급**: [nhplug.com](https://www.nhplug.com) → API 사용신청 → 앱키(APPKEY)·시크릿(APP SECRET), 유효기간 1년 (3개월 미거래 시 자동해지 가능)
- **Base URL(운영)**: `https://api.nhplug.com:8443` — N2 계정은 별도 도메인(`NAMU_API_BASE_URL`로 지정)
- **호출 제한**: 초당 약 5회 (자동 재시도 없음 — 어댑터도 재시도/스로틀 없음)
- **출처**: 공식 NH PLUG 공통 OpenAPI(`v1`) + 커뮤니티 Rust SDK [`nhplug-rs`](https://github.com/CHO-GGERNAUT/nhplug-rs)(MIT)가 문서화한 동일 와이어
- **확정 여부**: 라이브 프로브(`--live`)가 최종 권위. 아래 필드명은 공식 스펙 기준이며, 프로브(`apps/server/src/bin/verify_namu_account.rs`)에서 실측 확인 예정

## 0. 인증 — OAuth 접근토큰 발급

`POST /oauth2/token` (operationId `commonAuthIssueToken`)

- Content-Type: `application/x-www-form-urlencoded`
- **form body** (URL 쿼리에 시크릿을 절대 실지 않는다 — 프록시·접근 로그 기록 방지):
  - `appkey` = 앱키
  - `appsecretkey` = 시크릿
  - `grant_type` = `client_credentials`
  - `scope` = `oob`
- 응답: `{"access_token": "...", "expires_in": 86400}` — 토큰 24시간 유효
- 이후 모든 호출 헤더:
  - `Authorization: Bearer <access_token>`
  - `x-client-id: <appkey>`
  - `x-client-secret: <appsecret>`
  - `Content-Type: application/json; charset=UTF-8`
- 모의투자(moapi) 서버는 토큰을 발급하지 않는다 — 토큰은 운영 인증 서버에서

## 공통 응답 봉투

- HTTP 200이어도 업무 코드 `rsp_cd`로 성공/실패를 구분한다
- 성공 코드: `00000` (기본), `00166`/`00221`/`13578` (일부 조회 업무)
- 데이터 위치: `Output_0` (단일 객체 또는 목록), `Output_1` (행 목록)
- 요청 데이터 위치: `Input_0` 객체 (잔고·시세 POST)
- 실패 시: `rsp_cd`(게이트웨이/업무 코드) + `rsp_msg` — 아래 오류 코드 표 참고

## 1. 계좌목록 조회

`GET /n2/acctinfo` (operationId `commonAccountList`)

- 요청: 인증 헤더만 (본문 없음)
- 응답 `Output_0[]`: `acct_no`(계좌번호), `acct_type`(유형코드), `acct_name`(계좌명)
- 어댑터 매핑: `normalize_account` → `NamuAccount { id, name }`

## 2. 국내주식 잔고(보유) 조회

`POST /krstock/inquiry/v1/balance` (operationId `krstockInquiryBalance`, 주식잔고조회)

- 요청 본문 `Input_0`:
  | 필드 | 의미 | 어댑터 값 |
  |---|---|---|
  | `act_no` | 계좌번호 (11자리, `/n2/acctinfo`의 `acct_no`) | 대상 계좌 |
  | `bnc_bse_cd` | 잔고기준코드 (1: 주식관련 총평가·체결기준 / 5: 주식잔고평가·현재가기준) | `1` |
  | `ltg_aot_dit_cd` | 상장폐지구분코드 (1: 상장종목 / 9: 전체) | `1` |
  | `aet_bse` | 자산기준 (1: 순자산 / 2: 총자산) | `1` |
  | `qut_dit_cd` | 시세구분코드 (UNT: 통합 / KRX / NXT) | `UNT` |
- 응답:
  - `Output_0`: 계좌 요약 — `dca`(예수금), `nas_amt`(순자산), `tot_aet_amt`(총자산), `tot_eal_amt`(총평가), `tot_eal_pls`(총평가손익) 등
  - `Output_1[]`: 보유 종목 행
    - `iem_cd`(종목코드), `iem_nm`(종목명)
    - `rsdl_qty`(잔량수량), `phs_pr`(매입가격), `now_pr`(현재가격), `evlu_amt`(평가금액)
- 어댑터 매핑: `normalize_holding` → `NamuHolding { symbol, quantity, average_price, market_price, market_value }`
  - 숫자는 문자열/숫자 양쪽 수용 (`Decimal::from_str`, 쉼표·공백 제거)
  - `evlu_amt` 누락 시 수량×현재가로 대체
  - **수량 0 행은 제외** (`retain_nonzero_holdings`)

## 3. 국내주식 현재가 시세

`POST /krstock/quote/v1/currentPrice` (operationId `krstockQuoteCurrentPrice`, 주식현재가 시세)

- 요청 본문 `Input_0`: `iem_cd`(종목코드), `market_cd`(예: `KRX`)
- 응답 `Output_0`: `iem_cd`, `stck_prpr`(현재가)
- 어댑터 매핑: `normalize_quote` → `NamuQuote { symbol, market_price, currency }`
  - `currency`는 라이브 프로브에서 통화 필드 확인 후 채움 (현재 `None`)

## 호출하지 않는 API — 읽기 전용 보장

주문 계열 8개는 **호출 금지**이며, 테스트(`crates/connect/tests/namu_adapter_test.rs`)가
기록된 모든 요청 경로를 검사해 보장한다 (order/buy/sell/cancel/reserve 포함 금지 + 4개 읽기 endpoint 허용목록):

- `/krstock/order/v1/cashBuy`, `cashSell`, `creditBuy`, `creditSell`
- `/krstock/order/v1/modify`, `cancel`, `reservedOrder`, `reservedCancel`
- 그 밖의 미사용: 매수·매도가능수량(`buyableQuantity`/`sellableQuantity`), 손익 계열(`dailyPnl` 등), 해외주식·파생·채권·금현물 전체, WebSocket 채널 전체

## 게이트웨이 오류 코드 (공식 문서 기준)

| 코드 | 의미 | 어댑터 매핑 |
|---|---|---|
| `IGW40031` / `IGW40032` | 유효하지 않은 AppKey / AppSecret | `Unauthorized` |
| `IGW40037` | 유효하지 않은 grant | `Unauthorized` |
| `IGW40043` / `IGW40044` | 유효하지 않은 / 만료된 token | `Unauthorized` |
| `IGW40301` | API 사용 권한 없음 | `Unauthorized` |
| `IGW42901`~`IGW42903` | 호출 거래건수 초과 (HTTP 429, `Retry-After` 헤더) | `Transport("429")` |
| `IGW500xx` | 요청 처리·타임아웃·일시적 서버 오류 | `Transport(status)` |

- HTTP 401 → `Unauthorized`, 그 외 비성공 상태 → `Transport(status)` (상태만, 본문 제외)
- `rsp_msg`에 `연금`/`퇴직` 마커 포함 시 `AccountUnsupported` — 마커는 라이브 프로브에서 확정·보강 예정

## 코드 위치

| 역할 | 파일 |
|---|---|
| 경로 상수 | `crates/connect/src/broker/namu/mod.rs` (`paths` 모듈) |
| HTTP 전송 · 오류 매핑 | `crates/connect/src/broker/namu/client.rs` |
| 와이어 DTO · 정규화 | `crates/connect/src/broker/namu/models.rs` |
| 읽기 전용 계약 (3개 메서드) | `crates/connect/src/broker/namu/service.rs` |
| 오류 분류 (`classify_business_failure`) | `crates/connect/src/broker/namu/error.rs` |
| 실행 프로브 (fixture/`--live`) | `apps/server/src/bin/verify_namu_account.rs` |

## 다음 단계 후보 (아직 미사용)

- **ETF 구성종목**: `GET /krstock/quote/v1/etfComponents` — look-through(국가·섹터·통화) 분석용
- **기간별 시세**: `GET /krstock/quote/v1/period` — 가격 시계열
- **자산현황**: `GET /krstock/inquiry/v1/assetStatus` — 계좌 자산 요약
- 위는 ETF Universe 단계(별도 플랜)에서 라이브 증명 이후에만 활용한다.