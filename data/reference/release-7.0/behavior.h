/*
 * behavior.h
 *
 *  Created on: Dec 31, 2009
 *      Author: Rob Shillingsburg
 */

#ifndef BEHAVIOR_H_
#define BEHAVIOR_H_

#include <string>

using std::string;

#include "gameobject.h"
#include "xml.h"

class GameObject;
class Tile;
class Timer;
class BehaviorCreatorBase;

class StatBoostedBehavior;
class ShadowBehavior;

enum ShootType {
	TARGETED = 0,
	AUTO = 1,
	NUM_SHOOT_TYPES
};

class Behavior {
public:
	static Behavior *New(const XMLOptionedTag &kind, GameObject *host);

	virtual bool Update(double dt) { return false; }
	virtual void AtDeath(GameObject *shooter) {}
	virtual ~Behavior() {}
	virtual void Clear() {}

	virtual StatBoostedBehavior *AsStatBoostedBehavior() { return NULL; }
	virtual ShadowBehavior *AsShadowBehavior() { return NULL; }

	const string &bucket() const { return bucket_; }
	bool closed() const { return closed_; }
protected:
	static unordered_map<string, BehaviorCreatorBase*>* behaviorNameMap_;
	static double GetFloatArgument(const XMLOptionsMap &options,
		const char* name, double defaultValue);
	static int GetIntArgument(const XMLOptionsMap &options,
		const char* name, int defaultValue);
	static uint GetHexIntArgument(const XMLOptionsMap &options,
		const char* name, uint defaultValue);
	static bool GetBooleanArgument(const XMLOptionsMap &options,
		const char* name, bool defaultValue);
	static string GetStringArgument(const XMLOptionsMap &options,
		const char* name, string defaultValue);

	Behavior(GameObject *host, const XMLOptionsMap& options);
	Behavior(GameObject *host) : host_(host), bucket_(""), closed_(false) { }

	int findClosest(double maxDist, int type);

	GameObject *host_;
	string bucket_;
	bool closed_;
};

class WanderBehavior : public Behavior {
public:
	WanderBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double speed_;  // tiles per tick
	bool avoidWater_;
};

class ChargeBehavior : public Behavior {
public:
	ChargeBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	void Charge(double dt);

	double cooldown_;
	double speed_;
	double range_;

	double cooldownLeft_;
	bool charging_;
	double currentDist_;
	WorldPosition targetPos_;
};

class FollowBehavior : public Behavior {
public:
	FollowBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	double acquireRange_;
	double range_;
	double speed_;    // tiles per tick
	double duration_; // seconds, or 0 for unlimited
	double cooldown_; // seconds between follows
	bool predictive_;

	bool following_;  // else cooling down
	double timeLeft_; // seconds left in current mode (follow or cooldown)
};

class ShootBehavior : public Behavior {
public:
	ShootBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	void Shoot(GameObject* target);
	void SetNewCooldown();

	ShootType shootType_;
	double range_;
	int numShots_;
	double cooldown_;
	bool cooldownJitter_;
	int projectileId_;
	int objectType_;
	double angleInc_;
	float predictive_;
	double defaultAngle_;
	double arc_;  // Only aims in this arc from facing
	bool blastEffect_;
	double offset_;

	double tilShot_;
};

class ChangeSizeBehavior : public Behavior {
public:
	ChangeSizeBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double rate_;  // per tick
	double maxSize_;
	double minSize_;
};

class DecayBehavior : public Behavior {
public:
	DecayBehavior(GameObject *host, const XMLOptionsMap &options);
	DecayBehavior(GameObject *host, double tilDeath);
	bool Update(double dt);
	void Clear();
private:
	double time_;
	string particleEffect_;
	uint color_;
	bool dropLoot_;
	bool giveXP_;

	double tilDeath_;
};

class MetamorphoseBehavior : public Behavior {
public:
	MetamorphoseBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	double time_;
	int childType_;
	bool dropLoot_;
	bool giveXP_;

	double tilMetamorphisis_;
};

class MetamorphoseAtDeathBehavior : public Behavior {
public:
	MetamorphoseAtDeathBehavior(GameObject *host, const XMLOptionsMap &options);
	MetamorphoseAtDeathBehavior(GameObject *host, const string &childId,
		float posJitter);
	void AtDeath(GameObject *shooter);
private:
	float prob_;
	float posJitter_;
	int childType_;
};

class ExplodeBehavior : public Behavior {
public:
	ExplodeBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	double initialDelay_;
	double range_;
	int numShots_;
	int arcDegrees_;
	int projectileId_;
	bool dropLoot_;
	bool giveXP_;

	double tilActivate_;
};

class ProtectBehavior : public Behavior {
public:
	ProtectBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	int FindProtectee();

	int protecteeType_;
	double acquireRange_;
	double protectionRange_;
	double reprotectRange_;
	double speed_;  // squares per tick

	bool reprotecting_;
	int protecteeId_;
	double tilFindProtectee_;
};

class VictoryAtDeathBehavior : public Behavior {
public:
	VictoryAtDeathBehavior(GameObject *host, const XMLOptionsMap &options);
	void AtDeath(GameObject *shooter);
private:
};


class TauntBehavior : public Behavior {
public:
	TauntBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void AtDeath(GameObject *shooter);
private:
	bool DoTaunt(const string &list);

	string list_;
	bool broadcast_;

	double sinceTaunt_;
	unordered_map<string, string> params_;
};

class SayBehavior : public Behavior {
public:
	SayBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	vector<string> texts_;
	bool broadcast_;
	double prob_;
	uchar bubbleTime_;

	bool done_;
};

class RemoveOnEmptyBehavior : public Behavior {
public:
	RemoveOnEmptyBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
};

class StayAboveBehavior : public Behavior {
public:
	StayAboveBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	IntPoint FindHighestPoint(int radius, int skip);
	int minAltitude_;
	double speed_;
	int initialLookRadius_;
	int lookIncrement_;

	int lookRadius_;
	double tilLook_;
	bool engage_;
	IntPoint dest_;
};

class StayInTerrainBehavior : public Behavior {
public:
	StayInTerrainBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	int ScoreTile(const Tile& t);
	IntPoint FindBestPoint(int radius);
	TerrainType terrain_;
	int minAltitude_, maxAltitude_;
	double speed_;
	int initialLookRadius_;
	int lookIncrement_;

	int lookRadius_;
	double tilLook_;
	bool engage_;
	IntPoint dest_;
};

class ReleaseSpawnAtDeathBehavior : public Behavior {
public:
	ReleaseSpawnAtDeathBehavior(GameObject *host,
		const XMLOptionsMap &options);
	void AtDeath(GameObject *shooter);
private:
	int min_;
	int max_;
	int childType_;
};

class MakeMinionsBehavior : public Behavior {
public:
	MakeMinionsBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	bool MakeChild();

	float cooldown_;
	bool cooldownJitter_;
	vector<int> childTypes_;
	uint maxChildren_;
	float minOffset_;
	float maxOffset_;

	double cooldownLeft_;
	vector<int> children_;
};

class OrbitBehavior : public Behavior {
public:
	OrbitBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	int FindProtectee();

	int protecteeType_;
	double acquireRange_;
	double speed_;     // squares per tick
	double radius_;
	double speedVariability_;
	double radiusVariability_;
	double radiusWobbleDistance_;
	double radiusWobblePeriod_;

	int protecteeId_;
	double tilFindProtectee_;
	int direction_;            // 1 or -1
	double phasePosition_;     // 0 to 1, radius wobble sine curve x position
};

class OryxTauntBehavior : public Behavior {
public:
	OryxTauntBehavior(GameObject *host, const XMLOptionsMap &options);
	void AtDeath(GameObject *shooter);
};

class HealBehavior : public Behavior {
public:
	HealBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	bool HealsNeeded(vector<GameObject*>* needHeals);
	void FindHealTargets();

	bool hideVisual_;
	unordered_set<int> healTargetIds_;
	double cooldown_;
	double range_;
	int maxTargets_;
	int maxHeal_;

	double findTargetsLeft_;
	vector<int> healTargets_;
	double cooldownLeft_;
};

class StayBackBehavior : public Behavior {
public:
	StayBackBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double distance_;
	double speed_;
};

class KeepDistanceBehavior : public Behavior {
public:
	KeepDistanceBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double acquireRange_;
	double distance_;
	double speed_;
};

class CircleBehavior : public Behavior {
public:
	CircleBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double acquireRange_;
	double distance_;
	double speed_;
	bool circleRight_;
};

class StayCloseToSpawn : public Behavior {
public:
	StayCloseToSpawn(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	WorldPosition pos_;
	double speed_;
	double range_;
	double cooldown_;

	bool engaged_;
	double cooldownLeft_;
};

class FollowOrdersBehavior : public Behavior {
public:
	FollowOrdersBehavior(GameObject *host, const XMLOptionsMap &options);
	~FollowOrdersBehavior();
	void Clear();
	bool Update(double dt);
private:
	int FindClosestListen();

	unordered_set<int> listenTypes_;
	double acquireRange_;

	int listenObjectId_;
	double cooldown_;
	double cooldownLeft_;
	const XMLState* orders_;
	State* state_;
};

class SwirlBehavior : public Behavior {
public:
	SwirlBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double speed_;
	double acceleration_;
	double turnRate_;
};

class BuzzBehavior : public Behavior {
public:
	BuzzBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	double speed_;
	double acceleration_;
	double turnRate_;
	double acquireRange_;
	double cooldown_;

	double tilFindTarget_;
	bool hasTarget_;
	WorldPosition target_;
	bool locked_;  // lined up for the run
	bool buzzed_;  // buzzed the target
};

class BackAndForthBehavior : public Behavior {
public:
	BackAndForthBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	double speed_;
	double acceleration_;
	double angle_;
	double turnRate_;
	double radius_;

	bool hasCenter_;
	WorldPosition center_;
	bool moveInward_;
	bool direction_;
	float currSpeed_;
};

class MoveBehavior : public Behavior {
public:
	MoveBehavior(GameObject *host, const XMLOptionsMap &options);
	MoveBehavior(GameObject *host, double speed, double distance, double angle,
		double cooldown);
	bool Update(double dt);
	void Clear();
private:
	double speed_;
	double distance_;
	double angle_;
	double cooldown_;

	double cooldownLeft_;
	WorldPosition target_;
};

class MaintainProtectorsBehavior : public Behavior {
public:
	MaintainProtectorsBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	WorldPosition ChildPos(int i);
	void MakeChildren();

	bool coolingDown_;

	int cooldown_;
	vector<int> childTypes_;
	string childCorpseId_;
	uint numChildren_;
	int distance_;

	double cooldownLeft_;
	vector<int> children_;
	vector<DamageRecord> records_;
};


class RemainVisibleBehavior : public Behavior {
public:
	RemainVisibleBehavior(GameObject *host, const XMLOptionsMap &options);
	void AtDeath(GameObject *shooter);
};


class PoisonGrenadeBehavior : public Behavior {
public:
	PoisonGrenadeBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);

	void SetOwnerId(int ownerId) { ownerId_ = ownerId; }
private:
	XMLOptionsMap behaviorOptions_;
	double tilDetonate_;
	double radius_;
	double duration_;
	int totalDamage_;

	int ownerId_;
};


class PoisonedBehavior : public Behavior {
public:
	PoisonedBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);

	void SetOwnerId(int ownerId) { ownerId_ = ownerId; }
private:
	void SendDamageMessage();

	int ownerId_;

	double timeLeft_;
	double damagePerSecond_;
	double sinceDamageMsg_;
	int recentDamage_;
};


class ZapBehavior : public Behavior {
public:
	ZapBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);

	void SetOwnerId(int ownerId) { ownerId_ = ownerId; }
private:
	double radius_;
	int damage_;
	double cooldown_;
	uint color_;
	vector<XMLConditionEffect> effects_;
	bool poison_;
	XMLOptionsMap poisonBehaviorOptions_;

	int ownerId_;

	double cooldownLeft_;
};

class HealZapBehavior : public Behavior {
public:
	HealZapBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double radius_;
	int healing_;
	double cooldown_;
	uint color_;

	double cooldownLeft_;
};

class TrapBehavior : public Behavior {
public:
	TrapBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);

	void SetOwnerId(int ownerId) { ownerId_ = ownerId; }
private:
	void BlowUp();

	double tilArmed_;
	double duration_;
	double radius_;
	double sensitivity_;
	int damage_;
	uchar condEffect_;
	float condDuration_;
	uint color_;

	int ownerId_;

	int ticksSincePulse_;
};

class TrapTriggerBehavior : public Behavior {
public:
	TrapTriggerBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	double duration_;
	double radius_;
	double radiusVariability_;
	int minDamage_;
	int maxDamage_;
	int soundId_;
	float posJitter_;
	vector<string> colorList_;
	string condEffect_;
	string condDuration_;
	float cooldown_;
	float cooldownJitter_;

	float tilFire_;
};

class ShadowBehavior : public Behavior {
public:
	ShadowBehavior(GameObject *host, const XMLOptionsMap &options);

	ShadowBehavior *AsShadowBehavior() { return this; }

	bool Update(double dt);
	void SetMaster(int id) { id_ = id; }
private:
	double range_;
	double speed_;
	double reshadowRange_;

	int id_;

	bool reshadowing_;
};

class GrenadeTossBehavior : public Behavior {
public:
	GrenadeTossBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	void Toss(const WorldPosition& pos);

	ShootType shootType_;
	double minRange_;
	double range_;
	double cooldown_;
	bool cooldownJitter_;
	double radius_;
	int damage_;
	uchar effect_;
	float duration_;
	double defaultAngle_;

	double tilShot_;
};

class GrenadeBehavior : public Behavior {
public:
	GrenadeBehavior(GameObject *host, float tilDetonate, float radius,
		float damage, uchar effect, float duration, int origType,
		const unordered_set<int>& playerIds);
	bool Update(double dt);
private:
	double tilDetonate_;
	double radius_;
	int damage_;
	uchar effect_;
	float duration_;
	int origType_;  // shooter's type
	unordered_set<int> playerIds_;
};

class ObjectTossBehavior : public Behavior {
public:
	ObjectTossBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();
private:
	void Toss(const WorldPosition& pos);

	ShootType shootType_;
	string childId_;
	double minRange_;
	double range_;
	double rangeJitter_;
	double cooldown_;
	bool cooldownJitter_;
	double defaultAngle_;
	bool centerObject_;

	double tilShot_;
};

class LootDuplicationBehavior : public Behavior {
public:
	LootDuplicationBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
private:
	double tilStopDuplicating_;
};

class WalkBehavior : public Behavior {
public:
	WalkBehavior(GameObject *host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	double speed_;
	unordered_set<int> groundTypes_;

	double timeLeft_;
	WorldPosition targetPos_;
};

class StatBoostedBehavior : public Behavior {
public:
	StatBoostedBehavior(GameObject *host, int stat, int amount, double duration);
	bool Update(double dt);

	StatBoostedBehavior *AsStatBoostedBehavior() { return this; }

	int stat() const { return stat_; }
private:
	int stat_;
	int amount_;
	double timeLeft_;
};

class ShowEffectBehavior : public Behavior {
public:
	ShowEffectBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	uchar effectType_;
	int color_;
	float flashPeriod_;
	int flashRepeats_;

	bool sent_;
};

class ConditionEffectBehavior : public Behavior {
public:
	ConditionEffectBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	uchar effect_;
	double duration_;

	bool started_;
};

class SetAltTextureBehavior : public Behavior {
public:
	SetAltTextureBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	char altTextureId_;

	bool started_;
};

class SetTagBehavior : public Behavior {
public:
	SetTagBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);

private:
	string tagName_;
};

class ClearTagBehavior : public Behavior {
public:
	ClearTagBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);

private:
	string tagName_;
};

class SetGlobalTagBehavior : public Behavior {
public:
	SetGlobalTagBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	double notificationCooldown_;
	double notificationCooldownLeft_;
	int notificationType_;
	bool sendNotification_;
	string tagName_;
};

class ClearGlobalTagBehavior : public Behavior {
public:
	ClearGlobalTagBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);

private:
	string tagName_;
};

class RestoresBreathBehavior : public Behavior {
public:
	RestoresBreathBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);

private:
	double radius_;
	double radiusSq_;
	double speed_;
};

class CopyDamageRecordBehavior : public Behavior {
public:
	CopyDamageRecordBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);

private:
	int targetType_;
	double range_;

	double sinceCopy_;
	int targetId_;
};

class ChangeGroundBehavior : public Behavior {
public:
	ChangeGroundBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);

private:
	int groundType_;
};

class PlaySoundBehavior : public Behavior {
public:
	PlaySoundBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	char soundId_;

	bool played_;
};

class PlaceMapBehavior : public Behavior {
public:
	PlaceMapBehavior(GameObject* host, const XMLOptionsMap &options);
	bool Update(double dt);
	void Clear();

private:
	int placeLeft_; // -1 means infinite
	double cooldown_; // 0.0 means it will be run only once
	double cooldownLeft_;
	string mapFile_;
};

class DeleteObjectBehavior : public Behavior {
public:
	DeleteObjectBehavior(GameObject* host, const XMLOptionsMap &options);
	~DeleteObjectBehavior();
	bool Update(double dt);
	void Clear();

private:
	bool isDeleted_;
	unordered_set<int>* objectTypes_;
	double radius_;
};
#endif /* BEHAVIOR_H_ */
