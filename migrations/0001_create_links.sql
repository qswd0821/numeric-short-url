-- 링크: 단축 번호와 원본 URL의 불변 연결.
--
-- 단축 번호(id)는 DB의 IDENTITY 시퀀스가 발급한다. 애플리케이션은 번호를 계산하지 않으므로
-- 동시에 여러 요청이 들어와도 같은 번호가 두 번 발급되지 않는다. PRIMARY KEY 제약이 중복을 최종 차단한다.
-- GENERATED ALWAYS 이므로 INSERT 시 번호를 직접 지정할 수 없다(번호 재사용 방지).
CREATE TABLE links (
    id           BIGINT GENERATED ALWAYS AS IDENTITY (START WITH 1) PRIMARY KEY,
    original_url TEXT        NOT NULL CHECK (char_length(original_url) BETWEEN 1 AND 2048),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 생성 후 불변: 어떤 경로로든 UPDATE / DELETE / TRUNCATE 를 거부한다.
CREATE FUNCTION reject_link_change() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'links are immutable: % is not allowed', TG_OP
        USING ERRCODE = 'restrict_violation';
END;
$$;

CREATE TRIGGER links_immutable_rows
    BEFORE UPDATE OR DELETE ON links
    FOR EACH ROW EXECUTE FUNCTION reject_link_change();

CREATE TRIGGER links_immutable_table
    BEFORE TRUNCATE ON links
    FOR EACH STATEMENT EXECUTE FUNCTION reject_link_change();
