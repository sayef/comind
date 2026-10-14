from sqlalchemy import text


def count_skills(conn):
    return conn.execute(text("SELECT count(*) FROM skills")).scalar()
